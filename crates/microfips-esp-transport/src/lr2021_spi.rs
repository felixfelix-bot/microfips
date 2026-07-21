//! LR2021 SPI driver — register-level communication with the Semtech LR2021 radio.
//!
//! This module provides a trait-based abstraction (`Lr2021Radio`) that decouples
//! the transport layer from the specific SPI/GPIO implementation. This allows:
//!
//! 1. Mock implementation for unit testing (no hardware required)
//! 2. esp-hal implementation for ESP32-C3 (real hardware)
//! 3. std implementation for simulator/testing on host
//!
//! ## LR2021 SPI Protocol Summary
//!
//! The LR2021 uses a half-duplex SPI interface with a BUSY handshake:
//!
//! 1. Wait for BUSY pin LOW
//! 2. Assert CS LOW
//! 3. Send command/address byte (bit 7: R/W, bits 6-0: address)
//! 4. Send/receive data bytes
//! 5. Deassert CS HIGH
//!
//! Commands are short opcodes sent without an address:
//! - 0x00: SET_STANDBY
//! - 0x03: SET_TX (with timeout params)
//! - 0x08: SET_RX (with timeout params)
//! - 0x16: CLEAR_IRQ_STATUS
//! - 0x17: GET_IRQ_STATUS
//! - 0x1F: CLEAR_RX_BUFFER / SET_BUFFER_BASE_ADDRESS
//!
//! Register access uses R/W bit:
//! - Read:  [0x1B | addr] then NOP reads
//! - Write: [0x0D | addr] then data writes
//!
//! ## FLRC Packet Structure
//!
//! FLRC packets include:
//! - Preamble (configurable, default 32 bits)
//! - Sync Word (4 bytes, must match TX/RX)
//! - Payload (variable, up to 255 bytes)
//! - CRC (2 bytes, optional)
//!
//! The radio handles preamble/sync/CRC in hardware. We only provide
//! and receive the payload bytes.

use core::fmt::Debug;

// ── Error type ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lr2021Error {
    /// SPI bus error (transfer failed)
    SpiError,
    /// Radio not responding (BUSY stuck HIGH, no IRQ)
    Timeout,
    /// Invalid configuration parameter
    InvalidConfig,
    /// CRC check failed on received packet
    CrcMismatch,
    /// Packet larger than MAX_PACKET
    PacketTooLong,
    /// Radio in wrong state for requested operation
    WrongState,
}

// ── Configuration ──────────────────────────────────────────────────

/// FLRC modulation parameters (from proven balloon baseline).
#[derive(Debug, Clone, Copy)]
pub struct Lr2021Config {
    /// Operating frequency in MHz (e.g., 2440.0 for 2.4 GHz ISM)
    pub freq_mhz: f32,
    /// FLRC bitrate in kbps (supported: 2600, 1300, 650, 325)
    pub bitrate_kbps: u32,
    /// TX power in dBm (range: -18 to +12)
    pub tx_power_dbm: i8,
    /// Sync word bytes (must match on TX and RX)
    pub sync_word: [u8; 4],
    /// Enable CRC (recommended)
    pub crc_enabled: bool,
    /// Maximum payload length per packet (FLRC: max 255)
    pub payload_length: u8,
}

impl Default for Lr2021Config {
    fn default() -> Self {
        // Proven baseline from balloon-range-tests Track 1
        Self {
            freq_mhz: 2440.0,
            bitrate_kbps: 2600,
            tx_power_dbm: 12,
            sync_word: [0x12, 0xAD, 0x10, 0x1B],
            crc_enabled: true,
            payload_length: 255,
        }
    }
}

// ── IRQ and Status ─────────────────────────────────────────────────

bitflags::bitflags! {
    /// IRQ source flags from the LR2021 GET_IRQ_STATUS command.
    /// These are 32-bit flags from the LR2021's 4-byte IRQ status register.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct IrqSource: u32 {
        const TX_DONE          = 0x00080000; // Bit 19
        const RX_DONE          = 0x00040000; // Bit 18
        const CMD_ERROR        = 0x00020000; // Bit 17
        const CRC_ERROR        = 0x00100000; // Bit 20
        const TIMEOUT          = 0x00200000; // Bit 21
        const PREAMBLE_DETECTED = 0x00000002; // Bit 1
        const SYNCWORD_VALID   = 0x00000004; // Bit 2
        const HEADER_VALID     = 0x00000008; // Bit 3
        const ALL              = 0xFFFFFFFF;
    }
}

/// Status of a received packet.
#[derive(Debug, Clone, Copy)]
pub struct PacketStatus {
    /// Number of payload bytes received
    pub length: usize,
    /// RSSI of the received packet (dBm, negative)
    pub rssi_dbm: i16,
    /// SNR of the received packet (dB)
    pub snr_db: i8,
    /// CRC passed
    pub crc_ok: bool,
}

impl Default for PacketStatus {
    fn default() -> Self {
        Self {
            length: 0,
            rssi_dbm: -127,
            snr_db: -127,
            crc_ok: true,
        }
    }
}

// ── Radio Trait ────────────────────────────────────────────────────

/// Abstract LR2021 radio interface.
///
/// Implemented by:
/// - `MockLr2021Radio` (unit tests, no hardware)
/// - `EspHalLr2021Radio` (ESP32-C3 with esp-hal, real hardware)
///
/// All methods are async to support the Embassy executor.
pub trait Lr2021Radio: Sized {
    /// Initialize the radio with the given FLRC configuration.
    /// Resets the chip, configures modulation, frequency, sync word, etc.
    async fn init(&mut self, config: &Lr2021Config) -> Result<(), Lr2021Error>;

    /// Start receiving (enter RX mode).
    async fn start_rx(&mut self) -> Result<(), Lr2021Error>;

    /// Send a packet (enters TX mode, transmits, returns after TX_DONE).
    /// Packet data must be ≤ MAX_PACKET (255) bytes.
    async fn send_packet(&mut self, data: &[u8]) -> Result<(), Lr2021Error>;

    /// Read a received packet from the radio's buffer.
    /// Returns the packet data and status (RSSI, SNR, CRC).
    async fn read_packet(&self, buf: &mut [u8]) -> Result<PacketStatus, Lr2021Error>;

    /// Get the current IRQ status flags.
    async fn get_irq_status(&self) -> Result<IrqSource, Lr2021Error>;

    /// Clear IRQ status flags (write 1 to clear).
    async fn clear_irq(&mut self) -> Result<(), Lr2021Error>;

    /// Check if the IRQ pin is asserted (for polling mode).
    /// Returns true if an interrupt is pending.
    async fn check_irq(&self) -> Result<bool, Lr2021Error>;

    /// Put the radio into standby mode (low power, quick wakeup).
    async fn standby(&mut self) -> Result<(), Lr2021Error>;

    /// Put the radio to sleep (lowest power, needs re-init on wake).
    async fn sleep(&mut self) -> Result<(), Lr2021Error>;
}

// ── Mock Implementation (for unit tests) ───────────────────────────

#[cfg(any(test, feature = "mock"))]
pub mod mock {
    use super::*;
    use core::cell::RefCell;
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use embassy_sync::mutex::Mutex;

    /// Mock LR2021 radio for unit testing.
    ///
    /// Stores TX packets in a buffer that can be inspected by the test.
    /// RX packets are pre-loaded via `load_rx_packet()`.
    pub struct MockLr2021Radio {
        tx_packets: Mutex<CriticalSectionRawMutex, alloc::vec::Vec<alloc::vec::Vec<u8>>>,
        rx_queue: Mutex<CriticalSectionRawMutex, alloc::vec::Vec<alloc::vec::Vec<u8>>>,
        irq_flags: Mutex<CriticalSectionRawMutex, IrqSource>,
        initialized: Mutex<CriticalSectionRawMutex, bool>,
        config: Mutex<CriticalSectionRawMutex, Option<Lr2021Config>>,
    }

    impl MockLr2021Radio {
        pub fn new() -> Self {
            Self {
                tx_packets: Mutex::new(alloc::vec::Vec::new()),
                rx_queue: Mutex::new(alloc::vec::Vec::new()),
                irq_flags: Mutex::new(IrqSource::empty()),
                initialized: Mutex::new(false),
                config: Mutex::new(None),
            }
        }

        /// Pre-load a packet that will be returned on the next `read_packet()` call.
        /// Also sets RX_DONE IRQ flag.
        pub async fn load_rx_packet(&self, data: &[u8]) {
            self.rx_queue
                .lock()
                .await
                .push(data.to_vec());
            let mut flags = self.irq_flags.lock().await;
            *flags |= IrqSource::RX_DONE;
        }

        /// Get all packets that were transmitted (for test assertions).
        pub async fn get_tx_packets(&self) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
            self.tx_packets.lock().await.clone()
        }

        /// Get the last configuration passed to init().
        pub async fn get_config(&self) -> Option<Lr2021Config> {
            *self.config.lock().await
        }

        /// Check if init() was called.
        pub async fn is_initialized(&self) -> bool {
            *self.initialized.lock().await
        }
    }

    impl Default for MockLr2021Radio {
        fn default() -> Self {
            Self::new()
        }
    }

    #[derive(Debug)]
    pub struct MockError;

    impl From<MockError> for Lr2021Error {
        fn from(_: MockError) -> Self {
            Lr2021Error::SpiError
        }
    }

    impl Lr2021Radio for MockLr2021Radio {
        async fn init(&mut self, config: &Lr2021Config) -> Result<(), Lr2021Error> {
            *self.config.lock().await = Some(*config);
            *self.initialized.lock().await = true;
            Ok(())
        }

        async fn start_rx(&mut self) -> Result<(), Lr2021Error> {
            Ok(())
        }

        async fn send_packet(&mut self, data: &[u8]) -> Result<(), Lr2021Error> {
            self.tx_packets
                .lock()
                .await
                .push(data.to_vec());
            // Simulate TX_DONE
            let mut flags = self.irq_flags.lock().await;
            *flags |= IrqSource::TX_DONE;
            Ok(())
        }

        async fn read_packet(&self, buf: &mut [u8]) -> Result<PacketStatus, Lr2021Error> {
            let mut rx = self.rx_queue.lock().await;
            if let Some(pkt) = rx.first() {
                let n = pkt.len().min(buf.len());
                buf[..n].copy_from_slice(&pkt[..n]);
                rx.remove(0);
                Ok(PacketStatus {
                    length: n,
                    crc_ok: true,
                    ..Default::default()
                })
            } else {
                Ok(PacketStatus::default())
            }
        }

        async fn get_irq_status(&self) -> Result<IrqSource, Lr2021Error> {
            Ok(*self.irq_flags.lock().await)
        }

        async fn clear_irq(&mut self) -> Result<(), Lr2021Error> {
            *self.irq_flags.lock().await = IrqSource::empty();
            Ok(())
        }

        async fn check_irq(&self) -> Result<bool, Lr2021Error> {
            Ok(!self.irq_flags.lock().await.is_empty())
        }

        async fn standby(&mut self) -> Result<(), Lr2021Error> {
            Ok(())
        }

        async fn sleep(&mut self) -> Result<(), Lr2021Error> {
            Ok(())
        }
    }
}

#[cfg(any(test, feature = "mock"))]
pub use mock::MockLr2021Radio;

// ── SPI Register Constants ─────────────────────────────────────────

/// SPI command opcodes (Semtech LR2021 datasheet).
pub mod commands {
    /// Set standby mode
    pub const SET_STANDBY: u8 = 0x00;
    /// Set TX mode (with timeout)
    pub const SET_TX: u8 = 0x03;
    /// Set RX mode (with timeout)
    pub const SET_RX: u8 = 0x08;
    /// Stop radio timer
    pub const STOP_TIMER: u8 = 0x0A;
    /// Set RX/TX fallback mode
    pub const SET_RX_TX_FALLBACK: u8 = 0x09;
    /// Write register command prefix
    pub const WRITE_REGISTER: u8 = 0x0D;
    /// Read register command prefix
    pub const READ_REGISTER: u8 = 0x1B;
    /// Write buffer (TX FIFO)
    pub const WRITE_BUFFER: u8 = 0x0E;
    /// Read buffer (RX FIFO)
    pub const READ_BUFFER: u8 = 0x1D;
    /// Clear IRQ status
    pub const CLEAR_IRQ_STATUS: u8 = 0x16;
    /// Get IRQ status
    pub const GET_IRQ_STATUS: u8 = 0x17;
    /// Get RX buffer status
    pub const GET_RX_BUFFER_STATUS: u8 = 0x13;
    /// Get packet status (RSSI, SNR)
    pub const GET_PACKET_STATUS: u8 = 0x14;
}

/// Important register addresses.
pub mod registers {
    /// Packet length (for FLRC variable-length mode)
    pub const PACKET_LENGTH: u16 = 0x903;
    /// TX modulation parameters
    pub const TX_MODULATION: u16 = 0x880;
    /// RX modulation parameters
    pub const RX_MODULATION: u16 = 0x884;
    /// Frequency synthesis (PLL)
    pub const PLL_FREQ: u16 = 0x88C;
    /// TX power amplifier
    pub const TX_POWER: u16 = 0x894;
    /// Sync word (first 4 bytes)
    pub const SYNC_WORD_0: u16 = 0x9C8;
    /// IRQ mask (enable/disable individual IRQs)
    pub const IRQ_MASK: u16 = 0x914;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let c = Lr2021Config::default();
        assert_eq!(c.freq_mhz, 2440.0);
        assert_eq!(c.bitrate_kbps, 2600);
        assert_eq!(c.tx_power_dbm, 12);
        assert_eq!(c.sync_word, [0x12, 0xAD, 0x10, 0x1B]);
        assert!(c.crc_enabled);
        assert_eq!(c.payload_length, 255);
    }

    #[test]
    fn test_irq_source_flags() {
        let irq = IrqSource::TX_DONE | IrqSource::RX_DONE;
        assert!(irq.contains(IrqSource::TX_DONE));
        assert!(irq.contains(IrqSource::RX_DONE));
        assert!(!irq.contains(IrqSource::CRC_ERROR));

        let empty = IrqSource::empty();
        assert!(empty.is_empty());
    }
}
