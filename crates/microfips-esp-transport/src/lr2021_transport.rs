//! LR2021 SPI transport adapter for the FIPS mesh protocol.
//!
//! Implements the `Transport` trait from `microfips-protocol` using the
//! LR2021 (Semtech sub-GHz/2.4GHz radio) over SPI, configured for FLRC
//! (Fast LoRa Communication) mode.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────┐     send()/recv()
//! │  FIPS Protocol   │ ◄──────────────────►  Lr2021Transport
//! │  (FrameWriter)   │                        │
//! └──────────────────┘                        │
//!                              ┌──────────────┴──────────────┐
//!                              │  TxFramer / RxFramer         │
//!                              │  (stream ↔ packet adapter)   │
//!                              └──────────────┬──────────────┘
//!                                             │
//!                              ┌──────────────┴──────────────┐
//!                              │  LR2021 SPI Driver           │
//!                              │  (register config + TX/RX)   │
//!                              └──────────────┬──────────────┘
//!                                             │ SPI bus
//!                              ┌──────────────┴──────────────┐
//!                              │  esp-hal::spi::Spi           │
//!                              │  + GPIO (CS, BUSY, DIO, RST) │
//!                              └──────────────────────────────┘
//! ```
//!
//! ## FLRC Configuration
//!
//! Based on proven baseline from balloon-range-tests (Track 1):
//! - Frequency: 2440 MHz (2.4 GHz ISM)
//! - Bitrate: 2600 kbps (FLRC)
//! - Payload: up to 255 bytes per packet
//! - TX Power: +12 dBm
//! - 0% packet loss at bench distance
//!
//! ## SPI Protocol
//!
//! The LR2021 uses a standard Semtech SPI interface:
//! - BUSY pin: must be LOW before any SPI transaction
//! - Read: `[0x1B | addr]` + NOP bytes for data
//! - Write: `[0x0D | addr]` + data bytes
//! - Commands: short opcodes (SET_STANDBY, SET_TX, SET_RX, etc.)
//! - IRQ: DIO pins signal TX_DONE / RX_DONE
//!
//! Reference: Semtech LR2021 datasheet, RadioLib v7.6.0 LR2021 module.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration, Timer};

use microfips_protocol::transport::Transport;

use crate::lr2021_framing::{RxFramer, TxFramer, MAX_PACKET};
use crate::lr2021_spi::{
    Lr2021Config, Lr2021Error, Lr2021Radio, IrqSource, PacketStatus,
};

/// Default timeout for radio operations (handshake, packet TX/RX).
const RADIO_TIMEOUT_MS: u64 = 5000;

/// Poll interval when waiting for IRQ.
const IRQ_POLL_MS: u64 = 1;

/// LR2021 Transport — implements `Transport` trait for FIPS mesh.
///
/// Wraps the LR2021 radio with the framing layer, providing a stream
/// interface (send/recv byte slices) over the packet-based FLRC link.
///
/// # Usage
///
/// ```ignore
/// let radio = Lr2021Radio::new(spi, cs, busy, dio, rst, config);
/// let transport = Lr2021Transport::new(radio);
/// // transport implements Transport — use with FrameWriter/FrameReader
/// ```
pub struct Lr2021Transport<R: Lr2021Radio> {
    pub radio: R,
    tx_framer: TxFramer,
    rx_framer: RxFramer,
    /// Signal set by IRQ handler when a packet is received
    rx_ready: Signal<CriticalSectionRawMutex, ()>,
    /// Signal set by IRQ handler when TX completes
    tx_done: Signal<CriticalSectionRawMutex, ()>,
    /// Flag: radio is initialized and ready
    initialized: bool,
}

impl<R: Lr2021Radio> Lr2021Transport<R> {
    /// Create a new LR2021 transport wrapper.
    ///
    /// The radio must be initialized (call `init()` before first use).
    pub fn new(radio: R) -> Self {
        Self {
            radio,
            tx_framer: TxFramer::new(),
            rx_framer: RxFramer::new(),
            rx_ready: Signal::new(),
            tx_done: Signal::new(),
            initialized: false,
        }
    }

    /// Initialize the radio with FLRC configuration.
    ///
    /// Must be called before any send/recv operations.
    /// Configures: frequency, bitrate, TX power, sync word, packet mode.
    pub async fn init(&mut self, config: &Lr2021Config) -> Result<(), Lr2021Error> {
        self.radio.init(config).await?;
        self.radio.start_rx().await?;
        self.initialized = true;
        Ok(())
    }

    /// Handle an IRQ from the radio's DIO pin.
    ///
    /// This should be called from the GPIO interrupt handler (or polled).
    /// It reads the IRQ status, clears flags, and signals the appropriate
    /// waiters (rx_ready for RX_DONE, tx_done for TX_DONE).
    pub async fn handle_irq(&mut self) -> Result<(), Lr2021Error> {
        let irq = self.radio.get_irq_status().await?;

        if irq.contains(IrqSource::RX_DONE) {
            // Read the received packet into the RX framer
            let mut pkt_buf = [0u8; MAX_PACKET];
            match self.radio.read_packet(&mut pkt_buf).await {
                Ok(status) => {
                    self.rx_framer.push_packet(&pkt_buf[..status.length]);
                    self.rx_ready.signal(());
                }
                Err(_) => {
                    // Packet read failed — clear RX and restart
                    self.radio.start_rx().await?;
                }
            }
        }

        if irq.contains(IrqSource::TX_DONE) {
            self.tx_done.signal(());
            // Return to RX mode after TX
            self.radio.start_rx().await?;
        }

        self.radio.clear_irq().await?;
        Ok(())
    }
}

/// Error type for the transport layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Radio hardware error (SPI failure, timeout, etc.)
    Radio(Lr2021Error),
    /// Radio not initialized — call init() first
    NotInitialized,
    /// Operation timed out (no packet received, TX never completed)
    Timeout,
    /// Received packet was corrupt (CRC mismatch)
    CrcError,
}

impl From<Lr2021Error> for TransportError {
    fn from(e: Lr2021Error) -> Self {
        TransportError::Radio(e)
    }
}

impl<R: Lr2021Radio> Transport for Lr2021Transport<R> {
    type Error = TransportError;

    async fn wait_ready(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(TransportError::NotInitialized);
        }
        // Give radio a moment to settle
        Timer::after(Duration::from_millis(10)).await;
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(TransportError::NotInitialized);
        }

        // Push data into TX framer — may need multiple radio packets
        let mut offset = 0;
        while offset < data.len() {
            let consumed = self.tx_framer.push(&data[offset..]);

            // If buffer is full, flush a packet to the radio
            if self.tx_framer.is_full() {
                let mut pkt = [0u8; MAX_PACKET];
                let n = self.tx_framer.take_packet(&mut pkt).unwrap();
                self.transmit_packet(&pkt[..n]).await?;
            }

            offset += consumed;
        }

        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.initialized {
            return Err(TransportError::NotInitialized);
        }

        // Try to drain from existing RX buffer first
        let n = self.rx_framer.drain(buf);
        if n > 0 {
            return Ok(n);
        }

        // No data buffered — wait for next packet via IRQ
        match with_timeout(
            Duration::from_millis(RADIO_TIMEOUT_MS),
            self.rx_ready.wait(),
        )
        .await
        {
            Ok(()) => {
                // Packet arrived — drain it
                let n = self.rx_framer.drain(buf);
                Ok(n)
            }
            Err(_) => Err(TransportError::Timeout),
        }
    }
}

impl<R: Lr2021Radio> Lr2021Transport<R> {
    /// Flush any pending TX data as a (possibly short) packet.
    /// Called when the upper layer wants to ensure data is transmitted
    /// immediately (e.g., before waiting for a response).
    pub async fn flush_tx(&mut self) -> Result<(), TransportError> {
        if self.tx_framer.has_pending() {
            let mut pkt = [0u8; MAX_PACKET];
            if let Some(n) = self.tx_framer.take_packet(&mut pkt) {
                self.transmit_packet(&pkt[..n]).await?;
            }
        }
        Ok(())
    }

    /// Transmit a single FLRC packet and wait for TX_DONE.
    async fn transmit_packet(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.radio.send_packet(data).await?;

        // Poll IRQ until TX_DONE (mock sets it immediately, real radio takes ~ms)
        for _ in 0..1000 {
            let irq = self.radio.get_irq_status().await?;
            if irq.contains(IrqSource::TX_DONE) {
                self.radio.clear_irq().await?;
                let _ = self.radio.start_rx().await;
                return Ok(());
            }
            embassy_futures::yield_now().await;
        }
        Err(TransportError::Timeout)
    }

    /// Poll the IRQ pin (alternative to interrupt-driven handle_irq).
    ///
    /// Call this in a loop if DIO interrupts are not configured.
    pub async fn poll_irq(&mut self) -> Result<(), TransportError> {
        if self.radio.check_irq().await? {
            self.handle_irq().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lr2021_spi::MockLr2021Radio;

    #[test]
    fn test_transport_initialization() {
        let radio = MockLr2021Radio::new();
        let transport = Lr2021Transport::new(radio);
        assert!(!transport.initialized);
    }
}
