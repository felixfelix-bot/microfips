//! ESP-NOW transport for FIPS mesh.
//!
//! Implements the `Transport` trait using ESP-NOW, a connectionless
//! peer-to-peer protocol on the WiFi MAC layer. No IP, no DHCP, no SSID.
//!
//! Path B hybrid: uses `esp-radio` for WiFi radio initialization (handles
//! NVS, netif, event loop, WiFi init/start internally) and `esp-radio`'s
//! safe `EspNow` wrapper for ESP-NOW operations (init, callbacks, peer
//! management, send/receive). No raw FFI needed — esp-radio wraps the
//! ESP-IDF `esp_now.h` and `esp_wifi.h` C APIs via `esp-wifi-sys`.

use core::fmt::Debug;

use esp_radio::esp_now::{
    EspNow, EspNowWifiInterface, PeerInfo, BROADCAST_ADDRESS, ESP_NOW_MAX_DATA_LEN,
};
use esp_radio::wifi::{ControllerConfig, SecondaryChannel};
use microfips_protocol::transport::Transport;

// ── constants ───────────────────────────────────────────────────────────────

/// Maximum data payload per ESP-NOW frame (250 bytes per ESP-NOW spec).
pub const ESP_NOW_PAYLOAD_MAX: usize = ESP_NOW_MAX_DATA_LEN;

/// Default WiFi channel for ESP-NOW mesh.
const ESPNOW_CHANNEL: u8 = 1;

/// MAC address length (6 bytes).
pub const MAC_LEN: usize = 6;



// ── error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum EspNowError {
    /// WiFi radio initialization failed.
    InitFailed,
    /// ESP-NOW send failed.
    SendFailed,
    /// ESP-NOW peer operation failed (add/remove/modify).
    PeerError,
    /// Transport not initialized.
    NotInitialized,
    /// Payload exceeds ESP-NOW max (250 bytes).
    PayloadTooLarge,
}

impl core::fmt::Display for EspNowError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InitFailed => f.write_str("ESP-NOW init failed"),
            Self::SendFailed => f.write_str("ESP-NOW send failed"),
            Self::PeerError => f.write_str("ESP-NOW peer error"),
            Self::NotInitialized => f.write_str("ESP-NOW not initialized"),
            Self::PayloadTooLarge => f.write_str("ESP-NOW payload too large"),
        }
    }
}

impl core::error::Error for EspNowError {}

// ── MAC address ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; MAC_LEN]);

impl MacAddress {
    pub const BROADCAST: MacAddress = MacAddress(BROADCAST_ADDRESS);

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() >= MAC_LEN {
            let mut mac = [0u8; MAC_LEN];
            mac.copy_from_slice(&b[..MAC_LEN]);
            Some(MacAddress(mac))
        } else {
            None
        }
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == BROADCAST_ADDRESS
    }
}

impl From<[u8; MAC_LEN]> for MacAddress {
    fn from(mac: [u8; MAC_LEN]) -> Self {
        MacAddress(mac)
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}



// ── ESP-NOW transport ───────────────────────────────────────────────────────

/// ESP-NOW transport for FIPS mesh.
///
/// Uses esp-radio for WiFi radio init and ESP-NOW management.
/// The `EspNow<'d>` handle is kept alive for the lifetime of this transport.
/// A background task polls the receiver and pushes packets into a channel.
pub struct EspNowTransport<'d> {
    esp_now: EspNow<'d>,
    /// The peer we send to by default (broadcast for mesh mode).
    default_peer: [u8; MAC_LEN],
    /// Marker to track initialization state.
    initialized: bool,
}

impl<'d> EspNowTransport<'d> {
    /// Initialize ESP-NOW using esp-radio WiFi init.
    ///
    /// This calls `esp_radio::wifi::new()` which handles:
    /// - NVS flash init
    /// - Network interface init + default event loop
    /// - WiFi radio init in station mode
    /// - WiFi start
    ///
    /// Then extracts the `EspNow` handle from the returned `Interfaces`.
    /// The `WifiController` is kept alive (dropping it would deinit WiFi).
    ///
    /// Returns `(transport, local_mac, wifi_controller)`. The caller must
    /// keep the `WifiController` alive for the lifetime of the transport.
    pub fn init(
        wifi: esp_hal::peripherals::WIFI<'d>,
    ) -> Result<(Self, MacAddress, esp_radio::wifi::WifiController<'d>), EspNowError> {
        // Initialize WiFi radio via esp-radio. This handles NVS, netif,
        // event loop, WiFi init, and WiFi start internally.
        // Use default ControllerConfig — ESP-NOW doesn't need AP/STA connection.
        let (mut controller, interfaces) =
            esp_radio::wifi::new(wifi, ControllerConfig::default()).map_err(|_| {
                #[cfg(feature = "log")]
                log::error!("esp_radio::wifi::new() failed");
                EspNowError::InitFailed
            })?;

        // Set WiFi channel for ESP-NOW
        controller
            .set_channel(ESPNOW_CHANNEL, SecondaryChannel::None)
            .map_err(|_| EspNowError::InitFailed)?;

        // Take the EspNow handle from the interfaces
        let esp_now = interfaces.esp_now;

        // Read our MAC address from the ESP-NOW peer info
        // esp-radio doesn't expose a direct "get local MAC" on EspNow,
        // but we can use esp_hal's MAC read.
        let local_mac = read_local_mac().unwrap_or(MacAddress([0u8; MAC_LEN]));

        #[cfg(feature = "log")]
        {
            log::info!("ESP-NOW initialized via esp-radio. MAC: {}", local_mac);
            log::info!("ESP-NOW channel: {}", ESPNOW_CHANNEL);
        }

        Ok((
            EspNowTransport {
                esp_now,
                default_peer: BROADCAST_ADDRESS,
                initialized: true,
            },
            local_mac,
            controller,
        ))
    }

    /// Register a peer for ESP-NOW communication.
    pub fn add_peer(&mut self, mac: MacAddress) -> Result<(), EspNowError> {
        if !self.initialized {
            return Err(EspNowError::NotInitialized);
        }
        let peer = PeerInfo {
            interface: EspNowWifiInterface::Station,
            peer_address: mac.0,
            lmk: None,
            channel: None,
            encrypt: false,
        };
        self.esp_now.add_peer(peer).map_err(|_| EspNowError::PeerError)
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, mac: MacAddress) -> Result<(), EspNowError> {
        if !self.initialized {
            return Err(EspNowError::NotInitialized);
        }
        self.esp_now
            .remove_peer(&mac.0)
            .map_err(|_| EspNowError::PeerError)
    }

    /// Send data to a specific peer via ESP-NOW.
    pub async fn send_to(&mut self, mac: &[u8; 6], data: &[u8]) -> Result<(), EspNowError> {
        if !self.initialized {
            return Err(EspNowError::NotInitialized);
        }
        if data.len() > ESP_NOW_PAYLOAD_MAX {
            return Err(EspNowError::PayloadTooLarge);
        }
        self.esp_now
            .send_async(mac, data)
            .await
            .map_err(|_| EspNowError::SendFailed)
    }

    /// Send a broadcast message to all ESP-NOW peers in range.
    pub async fn broadcast(&mut self, data: &[u8]) -> Result<(), EspNowError> {
        self.send_to(&BROADCAST_ADDRESS, data).await
    }

    /// Set the default peer for `Transport::send`.
    pub fn set_default_peer(&mut self, mac: MacAddress) {
        self.default_peer = mac.0;
    }

    /// Get the local MAC address (best-effort).
    pub fn local_mac(&self) -> MacAddress {
        read_local_mac().unwrap_or(MacAddress([0u8; MAC_LEN]))
    }
}

impl<'d> Transport for EspNowTransport<'d> {
    type Error = EspNowError;

    async fn wait_ready(&mut self) -> Result<(), Self::Error> {
        if self.initialized {
            Ok(())
        } else {
            Err(EspNowError::NotInitialized)
        }
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let peer = self.default_peer;
        self.send_to(&peer, data).await
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.initialized {
            return Err(EspNowError::NotInitialized);
        }

        // Poll the esp-radio receiver for incoming data.
        // EspNow::receive_async() returns a future that resolves with
        // ReceivedData when a packet arrives.
        let received = self.esp_now.receive_async().await;
        let data = received.data();
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);

        #[cfg(feature = "log")]
        log::trace!(
            "ESP-NOW recv: {} bytes from {:02x?}",
            n,
            received.info.src_address
        );

        Ok(n)
    }
}

// ── helper to get local MAC ─────────────────────────────────────────────────

/// Read the local WiFi STA MAC address from eFuse via esp-hal.
///
/// Uses esp-hal's pure-Rust eFuse reader (`interface_mac_address`) instead of
/// the ESP-IDF C symbol `esp_read_mac`. The C symbol lives in the ESP-IDF
/// `esp_wifi` component, which is NOT linked in the esp-radio/esp-rtos
/// bare-metal ecosystem — calling it produced an undefined-symbol link error.
pub fn read_local_mac() -> Result<MacAddress, EspNowError> {
    let mac = esp_hal::efuse::interface_mac_address(
        esp_hal::efuse::InterfaceMacAddress::Station,
    );
    let mut out = [0u8; MAC_LEN];
    let bytes = mac.as_bytes();
    if bytes.len() != MAC_LEN {
        return Err(EspNowError::InitFailed);
    }
    out.copy_from_slice(bytes);
    Ok(MacAddress(out))
}