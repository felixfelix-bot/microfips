//! ESP-NOW transport for FIPS mesh.
//!
//! Implements the `Transport` trait using ESP-NOW, a connectionless
//! peer-to-peer protocol on the WiFi MAC layer. No IP, no DHCP, no SSID.
//!
//! Uses raw FFI to the ESP-IDF `esp_now.h` C API (available via esp-rtos).
//! All ESP32-C3 boards in the mesh share the same WiFi channel (default 1).

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration, Timer};
use async_trait::async_trait;

use microfips_protocol::transport::Transport;

// ── ESP-IDF C FFI ───────────────────────────────────────────────────────────
// These bind against the ESP-IDF libraries linked by esp-rtos.

mod ffi {
    use core::ffi::{c_int, c_uchar, c_uint, c_void};

    pub const ESP_OK: c_int = 0;
    pub const ESP_NOW_ETH_ALEN: usize = 6;

    // ESP-IDF constants
    pub const ESP_ERR_NVS_NO_FREE_PAGES: i32 = 0x1103;
    pub const ESP_ERR_NVS_NEW_VERSION_FOUND: i32 = 0x1104;
    pub const WIFI_MODE_STA: i32 = 1;
    pub const ESP_MAC_WIFI_STA: i32 = 0;

    #[repr(C)]
    pub struct esp_now_peer_info {
        pub peer_addr: [c_uchar; ESP_NOW_ETH_ALEN],
        pub lmk: [c_uchar; 16],
        pub channel: c_uint,
        pub ifidx: c_uint,
        pub encrypt: bool,
        pub priv_padding: [c_uchar; 3],
    }

    pub type esp_now_send_cb_t = extern "C" fn(*const c_uchar, c_int);
    pub type esp_now_recv_cb_t = extern "C" fn(*const esp_now_recv_info, *const c_uchar, c_int);

    #[repr(C)]
    pub struct esp_now_recv_info {
        pub src_addr: *const c_uchar,
        pub des_addr: *const c_uchar,
        pub rx_ctrl: *mut c_void,
    }

    // WiFi init functions
    extern "C" {
        pub fn nvs_flash_init() -> c_int;
        pub fn nvs_flash_erase() -> c_int;
        pub fn esp_netif_init() -> c_int;
        pub fn esp_event_loop_create_default() -> c_int;
        pub fn esp_wifi_init(cfg: *const c_void) -> c_int;
        pub fn esp_wifi_set_mode(mode: c_int) -> c_int;
        pub fn esp_wifi_start() -> c_int;
        pub fn esp_wifi_set_channel(primary: c_uchar, secondary: c_uchar) -> c_int;
        pub fn esp_read_mac(mac: *mut c_uchar, typ: c_int) -> c_int;
    }

    // ESP-NOW functions
    extern "C" {
        pub fn esp_now_init() -> c_int;
        pub fn esp_now_deinit() -> c_int;
        pub fn esp_now_register_send_cb(cb: esp_now_send_cb_t) -> c_int;
        pub fn esp_now_register_recv_cb(cb: esp_now_recv_cb_t) -> c_int;
        pub fn esp_now_add_peer(peer: *const esp_now_peer_info) -> c_int;
        pub fn esp_now_del_peer(peer_addr: *const c_uchar) -> c_int;
        pub fn esp_now_send(
            peer_addr: *const c_uchar,
            data: *const c_uchar,
            len: usize,
        ) -> c_int;
        pub fn esp_now_get_peer(peer_addr: *const c_uchar, peer: *mut esp_now_peer_info) -> c_int;
        // esp_fill_random is available via ESP-IDF
        pub fn esp_fill_random(buf: *mut c_void, len: usize);
    }
}

// ── constants ───────────────────────────────────────────────────────────────

/// Maximum data payload per ESP-NOW frame (250 bytes minus 6-byte fragment header = 244).
pub const ESP_NOW_PAYLOAD_MAX: usize = 244;

/// Default WiFi channel for ESP-NOW mesh.
const ESPNOW_CHANNEL: u8 = 1;

/// MAC address length (6 bytes).
pub const MAC_LEN: usize = 6;

// ── error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum EspNowError {
    InitFailed,
    SendFailed,
    PeerNotFound,
    NoPeer,
    Timeout,
    NotInitialized,
}

// ── MAC address ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; MAC_LEN]);

impl MacAddress {
    pub const BROADCAST: MacAddress = MacAddress([0xff; MAC_LEN]);

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() >= MAC_LEN {
            let mut mac = [0u8; MAC_LEN];
            mac.copy_from_slice(&b[..MAC_LEN]);
            Some(MacAddress(mac))
        } else {
            None
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xff; MAC_LEN]
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

// ── global state ────────────────────────────────────────────────────────────

static ESPNOW_INITIALIZED: AtomicBool = AtomicBool::new(false);

// Signal for received data — wakes the recv task
type RecvSignal = Signal<CriticalSectionRawMutex, ()>;
static RECV_SIGNAL: RecvSignal = Signal::new();

// Packet structure for incoming ESP-NOW data
#[derive(Clone, Copy)]
struct Packet {
    mac: MacAddress,
    len: u8,
    data: [u8; ESP_NOW_PAYLOAD_MAX],
}

// Circular buffer for incoming packets with proper synchronization
const PACKET_QUEUE_SIZE: usize = 16;
static PACKET_QUEUE: embassy_sync::mutex::Mutex<
    CriticalSectionRawMutex, 
    heapless::spsc::Queue<Packet, PACKET_QUEUE_SIZE>
> = embassy_sync::mutex::Mutex::new(heapless::spsc::Queue::new());

// ── C callback trampolines ──────────────────────────────────────────────────

extern "C" fn esp_now_send_cb(_mac: *const u8, _status: i32) {
    // ESP-NOW send status: 0 = success, non-zero = fail
    // We don't block on send completion — fire-and-forget (Wirehair handles loss)
}

extern "C" fn esp_now_recv_cb(
    recv_info: *const ffi::esp_now_recv_info,
    data: *const u8,
    len: i32,
) {
    if recv_info.is_null() || data.is_null() || len <= 0 || len > ESP_NOW_PAYLOAD_MAX as i32 {
        return;
    }

    let src_mac = unsafe { (*recv_info).src_addr };
    let data_slice = unsafe { core::slice::from_raw_parts(data, len as usize) };

    // Use mutex to safely access the packet queue
    embassy_sync::mutex::Mutex::lock(&PACKET_QUEUE, |queue| {
        if queue.len() < PACKET_QUEUE_SIZE {
            // Create packet structure
            let mut packet = Packet {
                mac: MacAddress([0u8; MAC_LEN]),
                len: len as u8,
                data: [0u8; ESP_NOW_PAYLOAD_MAX],
            };
            
            // Copy MAC address
            unsafe {
                core::ptr::copy_nonoverlapping(src_mac, packet.mac.0.as_mut_ptr(), MAC_LEN);
            }
            
            // Copy data
            packet.data[..data_slice.len()].copy_from_slice(data_slice);
            
            // Enqueue the packet
            queue.push(packet).ok();
            
            // Signal the recv task
            RECV_SIGNAL.signal(());
        }
    });
}

// ── ESP-NOW transport ───────────────────────────────────────────────────────

pub struct EspNowTransport {
    initialized: bool,
    local_mac: MacAddress,
    peer: MacAddress,
}

impl EspNowTransport {
    /// Initialize ESP-NOW: WiFi STA mode → esp_now_init().
    /// Must be called before any send/recv.
    pub fn init() -> Result<(Self, MacAddress), EspNowError> {
        if ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            // Already initialized — return a new handle
            return Err(EspNowError::InitFailed);
        }

        // Packet queue is already initialized as static mutex

        unsafe {
            // 1. Init NVS
            let mut ret = ffi::nvs_flash_init();
            if ret == ffi::ESP_ERR_NVS_NO_FREE_PAGES || ret == ffi::ESP_ERR_NVS_NEW_VERSION_FOUND {
                ffi::nvs_flash_erase();
                ret = ffi::nvs_flash_init();
            }
            if ret != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 2. Init network interface + event loop
            if ffi::esp_netif_init() != 0 {
                return Err(EspNowError::InitFailed);
            }
            if ffi::esp_event_loop_create_default() != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 3. Init WiFi in STA mode
            // Use minimal config — no connection, just radio on
            let cfg = core::mem::zeroed();
            if ffi::esp_wifi_init(&cfg as *const _ as *const _) != 0 {
                return Err(EspNowError::InitFailed);
            }
            if ffi::esp_wifi_set_mode(ffi::WIFI_MODE_STA) != 0 {
                return Err(EspNowError::InitFailed);
            }
            if ffi::esp_wifi_start() != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 4. Set channel
            if ffi::esp_wifi_set_channel(ESPNOW_CHANNEL, 0) != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 5. Init ESP-NOW
            if ffi::esp_now_init() != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 6. Register callbacks
            let send_cb: ffi::esp_now_send_cb_t = esp_now_send_cb;
            let recv_cb: ffi::esp_now_recv_cb_t = esp_now_recv_cb;
            if ffi::esp_now_register_send_cb(send_cb) != 0 {
                return Err(EspNowError::InitFailed);
            }
            if ffi::esp_now_register_recv_cb(recv_cb) != 0 {
                return Err(EspNowError::InitFailed);
            }

            // 7. Read our MAC
            let mut mac_buf = [0u8; MAC_LEN];
            if ffi::esp_read_mac(mac_buf.as_mut_ptr(), ffi::ESP_MAC_WIFI_STA) != 0 {
                return Err(EspNowError::InitFailed);
            }
            let local_mac = MacAddress(mac_buf);
        }

        ESPNOW_INITIALIZED.store(true, Ordering::Release);

        Ok((
            EspNowTransport {
                initialized: true,
                local_mac: local_mac,
                peer: MacAddress([0u8; MAC_LEN]),
            },
            local_mac,
        ))
    }

    /// Register a peer for ESP-NOW communication.
    pub fn add_peer(&mut self, mac: MacAddress) -> Result<(), EspNowError> {
        if !ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            return Err(EspNowError::NotInitialized);
        }
        let peer = ffi::esp_now_peer_info {
            peer_addr: mac.0,
            lmk: [0u8; 16],
            channel: ESPNOW_CHANNEL as u32,
            ifidx: 0,
            encrypt: false,
            priv_padding: [0u8; 3],
        };
        let ret = unsafe { ffi::esp_now_add_peer(&peer as *const _) };
        if ret == 0 {
            self.peer = mac;
            Ok(())
        } else {
            Err(EspNowError::PeerNotFound)
        }
    }

    /// Remove a peer.
    pub fn del_peer(&mut self, mac: MacAddress) -> Result<(), EspNowError> {
        if !ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            return Err(EspNowError::NotInitialized);
        }
        let ret = unsafe { ffi::esp_now_del_peer(mac.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err(EspNowError::PeerNotFound)
        }
    }

    /// Send data to a peer via ESP-NOW.
    pub fn send_to(&self, mac: MacAddress, data: &[u8]) -> Result<(), EspNowError> {
        if !ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            return Err(EspNowError::NotInitialized);
        }
        if data.len() > ESP_NOW_PAYLOAD_MAX {
            return Err(EspNowError::SendFailed);
        }
        let ret = unsafe { ffi::esp_now_send(mac.as_ptr(), data.as_ptr(), data.len()) };
        if ret == 0 {
            Ok(())
        } else {
            Err(EspNowError::SendFailed)
        }
    }

    /// Send a broadcast message to all ESP-NOW peers in range.
    pub fn broadcast(&self, data: &[u8]) -> Result<(), EspNowError> {
        self.send_to(MacAddress::BROADCAST, data)
    }

    // Deinit ESP-NOW (not async-safe, for cleanup)
    pub fn deinit(&self) {
        if ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            unsafe {
                ffi::esp_now_deinit();
            }
            ESPNOW_INITIALIZED.store(false, Ordering::Release);
        }
    }
}

#[async_trait::async_trait]
impl Transport for EspNowTransport {
    type Error = EspNowError;

    async fn wait_ready(&mut self) -> Result<(), Self::Error> {
        if ESPNOW_INITIALIZED.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(EspNowError::NotInitialized)
        }
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.send_to(self.peer, data)
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // Check if there are any packets already in the queue
        let mut has_packet = false;
        embassy_sync::mutex::Mutex::lock(&PACKET_QUEUE, |queue| {
            has_packet = !queue.is_empty();
        });
        
        // Only wait for signal if queue is empty
        if !has_packet {
            // Wait for the signal from the ISR callback with a timeout
            match with_timeout(Duration::from_millis(1000), RECV_SIGNAL.wait()).await {
                Ok(_) => {}, // Signal received
                Err(_) => return Err(EspNowError::Timeout),
            }
        }
        
        // Lock the queue and dequeue a packet
        embassy_sync::mutex::Mutex::lock(&PACKET_QUEUE, |queue| {
            if let Some(packet) = queue.dequeue() {
                // Copy packet data to the buffer
                let len = packet.len as usize;
                let copy_len = len.min(buf.len());
                buf[..copy_len].copy_from_slice(&packet.data[..copy_len]);
                
                Ok(copy_len)
            } else {
                // No packet available (signal might be stale)
                Err(EspNowError::Timeout)
            }
        })
    }
}

impl Drop for EspNowTransport {
    fn drop(&mut self) {
        self.deinit();
    }
}

// ── helper to get local MAC without full transport ──────────────────────────

pub fn read_local_mac() -> Result<MacAddress, EspNowError> {
    let mut mac = [0u8; MAC_LEN];
    let ret = unsafe { ffi::esp_read_mac(mac.as_mut_ptr(), ffi::ESP_MAC_WIFI_STA) };
    if ret == 0 {
        Ok(MacAddress(mac))
    } else {
        Err(EspNowError::InitFailed)
    }
}
