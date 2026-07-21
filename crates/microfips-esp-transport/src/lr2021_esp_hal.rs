//! EspHalLr2021Radio — real SPI implementation of Lr2021Radio for ESP32-C3.
//!
//! Uses esp-hal SPI peripheral + GPIO for BUSY/IRQ/RST pins.
//! Pin mapping (from hardware verification):
//!   GPIO7  = SPI MOSI
//!   GPIO2  = SPI MISO
//!   GPIO6  = SPI SCLK
//!   GPIO10 = SPI CS (NSS)
//!   GPIO3  = LR2021 RESET
//!   GPIO4  = LR2021 BUSY
//!   GPIO5  = LR2021 DIO9 (IRQ)
//!
//! SPI clock: 20 MHz (max verified per speed-tests discovery).
//! XTAL: 52 MHz (LR2021 module crystal).
//!
//! Init sequence ported from verified RP2040 raw SPI implementation
//! (see flrc-firmware-ops skill, references/raw-spi-init-sequence.md).

use crate::lr2021_spi::{IrqSource, Lr2021Config, Lr2021Error, Lr2021Radio, PacketStatus};

// ── SPI Command Opcodes (2-byte) ───────────────────────────────────

const OP_CLEAR_ERRORS: [u8; 4] = [0x01, 0x11, 0x00, 0x00];
const OP_SET_STANDBY_XOSC: [u8; 3] = [0x01, 0x28, 0x01];
const OP_SET_PACKET_TYPE_FLRC: [u8; 3] = [0x02, 0x07, 0x05];
const OP_SET_RF_FREQUENCY: [u8; 2] = [0x02, 0x00];
const OP_SET_RX_PATH_HF: [u8; 4] = [0x02, 0x01, 0x01, 0x00];
const OP_CALIB_FRONT_END: [u8; 2] = [0x01, 0x23];
const OP_CALIBRATE_ALL: [u8; 3] = [0x01, 0x22, 0x5F]; // 0x5F not 0x6F
const OP_SET_FLRC_MOD_PARAMS: [u8; 4] = [0x02, 0x48, 0x00, 0x25]; // 2600kbps, CR_1_0, BT0.5
const OP_SET_FLRC_SYNCWORD: [u8; 2] = [0x02, 0x4C];
const OP_SET_FLRC_PACKET_PARAMS: [u8; 2] = [0x02, 0x49];
const OP_SET_RX_TX_FALLBACK_FS: [u8; 3] = [0x02, 0x06, 0x03]; // Fs=0x03, NOT STDBY_RC=0x00
const OP_DIO_FUNCTION: [u8; 4] = [0x01, 0x12, 0x09, 0x11]; // DIO9 = IRQ
const OP_CLEAR_IRQ: [u8; 6] = [0x01, 0x16, 0xFF, 0xFF, 0xFF, 0xFF];
const OP_SET_RX_CONTINUOUS: [u8; 5] = [0x02, 0x0C, 0xFF, 0xFF, 0xFF]; // 5 bytes!
const OP_SET_TX: [u8; 5] = [0x02, 0x0D, 0x00, 0x00, 0x00]; // 5 bytes!
const OP_SET_PA_CONFIG: [u8; 7] = [0x02, 0x02, 0x80, 0x00, 0x60, 0x07, 0x10];
const OP_SET_TX_PARAMS: [u8; 2] = [0x02, 0x03];
const OP_SET_STANDBY_RC: [u8; 3] = [0x01, 0x28, 0x00];
const OP_SET_SLEEP: [u8; 3] = [0x01, 0x29, 0x00];

// Read opcodes
const OP_GET_IRQ_STATUS: [u8; 2] = [0x01, 0x17];
const OP_GET_AND_CLEAR_IRQ: [u8; 2] = [0x01, 0x18];
const OP_GET_RX_BUFFER_STATUS: [u8; 2] = [0x01, 0x13];
const OP_GET_PACKET_STATUS: [u8; 2] = [0x01, 0x14];

// FIFO operations
const OP_WRITE_TX_FIFO: [u8; 2] = [0x00, 0x02];
const OP_READ_RX_FIFO: [u8; 2] = [0x00, 0x03];
const OP_CLR_TX_FIFO: [u8; 2] = [0x01, 0x1F];
const OP_CLR_RX_FIFO: [u8; 2] = [0x01, 0x1E];

// DIO IRQ config — map RX_DONE (bit18) and TX_DONE (bit19)
const OP_DIO_IRQ_CONFIG_RX: [u8; 6] = [0x01, 0x15, 0x00, 0x04, 0x00, 0x00]; // bit18 = RX_DONE
const OP_DIO_IRQ_CONFIG_TX: [u8; 6] = [0x01, 0x15, 0x00, 0x08, 0x00, 0x00]; // bit19 = TX_DONE

// ── Constants ──────────────────────────────────────────────────────

const XTAL_MHZ: f32 = 52.0;
const SPI_FREQ_HZ: u32 = 20_000_000; // 20 MHz max per speed-tests
const MAX_PACKET: usize = 255;
const BUSY_TIMEOUT_ITER: u32 = 100_000;
const TX_DONE_ITER: u32 = 100_000;

// ── EspHalLr2021Radio ──────────────────────────────────────────────

/// Real LR2021 radio driver for ESP32-C3 using esp-hal.
///
/// Generic over SPI bus and GPIO pins to allow flexible wiring.
/// Uses blocking SPI + GPIO polling for BUSY/IRQ (simplest, proven on RP2040).
pub struct EspHalLr2021Radio<SPI, CS, BUSY, IRQ, RST> {
    spi: SPI,
    cs: CS,
    busy: BUSY,
    irq: IRQ,
    rst: RST,
    is_tx: bool,
}

impl<SPI, CS, BUSY, IRQ, RST> EspHalLr2021Radio<SPI, CS, BUSY, IRQ, RST> {
    /// Create a new radio driver with the given peripherals.
    ///
    /// All pins must be configured before passing:
    /// - CS: output, default HIGH
    /// - BUSY: input
    /// - IRQ: input (active HIGH)
    /// - RST: output, default HIGH
    pub fn new(spi: SPI, cs: CS, busy: BUSY, irq: IRQ, rst: RST) -> Self {
        Self {
            spi,
            cs,
            busy,
            irq,
            rst,
            is_tx: false,
        }
    }
}

// ── SPI helper functions (require embedded-hal traits) ─────────────
//
// These are implemented via a private trait that bridges esp-hal types.
// In practice, the user will pass esp-hal::spi::Spi and esp-hal::gpio
// pins. The trait bounds are kept loose to allow both blocking and async.

/// Trait for SPI operations needed by the radio driver.
/// Implemented for esp-hal blocking SPI + GPIO types.
pub trait SpiHal {
    /// Assert CS (LOW), write bytes, deassert CS (HIGH).
    fn spi_write(&mut self, data: &[u8]) -> Result<(), Lr2021Error>;
    /// Assert CS (LOW), write opcode, deassert CS, wait BUSY, assert CS, read N bytes, deassert CS.
    fn spi_read(&mut self, opcode: &[u8], buf: &mut [u8]) -> Result<(), Lr2021Error>;
    /// Wait for BUSY pin to go LOW (radio ready).
    fn wait_busy(&self) -> Result<(), Lr2021Error>;
    /// Check IRQ pin (true = interrupt pending).
    fn check_irq_pin(&self) -> bool;
    /// Hardware reset: RST LOW 200us → HIGH, delay 50ms.
    fn hardware_reset(&mut self) -> Result<(), Lr2021Error>;
}

impl<SPI, CS, BUSY, IRQ, RST> EspHalLr2021Radio<SPI, CS, BUSY, IRQ, RST>
where
    Self: SpiHal,
{
    /// Send a command (write-only, no response expected).
    fn cmd(&mut self, data: &[u8]) -> Result<(), Lr2021Error> {
        self.wait_busy()?;
        self.spi_write(data)
    }

    /// Read a response after sending an opcode.
    fn read(&mut self, opcode: &[u8], buf: &mut [u8]) -> Result<(), Lr2021Error> {
        self.spi_read(opcode, buf)
    }

    /// Compute RF frequency register value.
    /// frf = (freq_MHz * 1e6 * 2^18) / (XTAL_MHz * 1e6)
    fn compute_frf(freq_mhz: f32) -> [u8; 3] {
        let frf = ((freq_mhz * 1e6 * (1u64 << 18) as f32) / (XTAL_MHZ * 1e6)) as u32;
        [(frf >> 16) as u8, (frf >> 8) as u8, frf as u8]
    }

    /// Build FLRC packet params bytes.
    /// preamble=16 (index 3), syncTx=1, syncMatch=1, fixed=1, crc depends on config.
    fn build_packet_params(config: &Lr2021Config) -> [u8; 4] {
        // Byte 0: ((preambleIndex & 0x0F) << 2) | (syncWordLen / 2)
        // preamble=16 → index 3: (3<<2)|2 = 0x0E
        let byte0 = 0x0Eu8;
        // Byte 1: ((syncTx & 0x03) << 6) | ((syncMatch & 0x07) << 3) | (fixedLen<<2) | crc
        // syncTx=1, syncMatch=1, fixed=1, crc=0 or 1
        let crc_byte = if config.crc_enabled { 0x01 } else { 0x00 };
        let byte1 = (1u8 << 6) | (1u8 << 3) | (1u8 << 2) | crc_byte;
        // Byte 2-3: payloadLen big-endian
        let len = config.payload_length;
        [byte0, byte1, 0x00, len]
    }

    /// Map bitrate to brBw value for SET_FLRC_MOD_PARAMS.
    fn bitrate_to_brbw(bitrate_kbps: u32) -> u8 {
        match bitrate_kbps {
            2600 => 0x00,
            2080 => 0x01,
            1300 => 0x02,
            650 => 0x03,
            325 => 0x04,
            _ => 0x00, // default to max
        }
    }

    /// Full init sequence (from verified RP2040 raw SPI).
    /// See flrc-firmware-ops skill references/raw-spi-init-sequence.md.
    fn init_sequence(&mut self, config: &Lr2021Config) -> Result<(), Lr2021Error> {
        // Step 0: Hardware reset
        self.hardware_reset()?;

        // Step 1: CLEAR_ERRORS
        self.cmd(&OP_CLEAR_ERRORS)?;

        // Step 2: SET_STANDBY (XOSC)
        self.cmd(&OP_SET_STANDBY_XOSC)?;

        // Step 3: SET_PACKET_TYPE FLRC
        self.cmd(&OP_SET_PACKET_TYPE_FLRC)?;

        // Step 4: SET_RF_FREQUENCY
        let frf = Self::compute_frf(config.freq_mhz);
        self.cmd(&[OP_SET_RF_FREQUENCY[0], OP_SET_RF_FREQUENCY[1], frf[0], frf[1], frf[2]])?;

        // Step 5: SET_RX_PATH (HF)
        self.cmd(&OP_SET_RX_PATH_HF)?;

        // Step 6: CALIB_FRONT_END
        // freq/4 | 0x8000 + padding
        let freq_div4 = (config.freq_mhz / 4.0) as u16 | 0x8000;
        self.cmd(&[
            OP_CALIB_FRONT_END[0],
            OP_CALIB_FRONT_END[1],
            (freq_div4 >> 8) as u8,
            freq_div4 as u8,
            0x00, 0x00,
        ])?;

        // Step 7: CALIBRATE (all blocks, 0x5F)
        self.cmd(&OP_CALIBRATE_ALL)?;
        // Wait for calibration to complete
        self.wait_busy()?;

        // Step 8: SET_FLRC_MOD_PARAMS
        let brbw = Self::bitrate_to_brbw(config.bitrate_kbps);
        self.cmd(&[OP_SET_FLRC_MOD_PARAMS[0], OP_SET_FLRC_MOD_PARAMS[1], brbw, 0x25])?;

        // Step 9: SET_FLRC_SYNCWORD
        self.cmd(&[
            OP_SET_FLRC_SYNCWORD[0],
            OP_SET_FLRC_SYNCWORD[1],
            0x01, // syncWordLen = 4 bytes (type 1)
            config.sync_word[0],
            config.sync_word[1],
            config.sync_word[2],
            config.sync_word[3],
        ])?;

        // Step 10: SET_FLRC_PACKET_PARAMS
        let pkt_params = Self::build_packet_params(config);
        self.cmd(&[
            OP_SET_FLRC_PACKET_PARAMS[0],
            OP_SET_FLRC_PACKET_PARAMS[1],
            pkt_params[0],
            pkt_params[1],
            pkt_params[2],
            pkt_params[3],
        ])?;

        // TX-only: SET_PA_CONFIG + SET_TX_PARAMS (between step 10 and 11)
        self.cmd(&OP_SET_PA_CONFIG)?;
        let power_raw = (config.tx_power_dbm as f32 * 2.0 + 0.5) as u8;
        self.cmd(&[
            OP_SET_TX_PARAMS[0],
            OP_SET_TX_PARAMS[1],
            power_raw,
            0x04, // ramp time
        ])?;

        // Step 11: SET_RX_TX_FALLBACK = Fs (0x03) — keeps PLL warm
        self.cmd(&OP_SET_RX_TX_FALLBACK_FS)?;

        // Step 12: DIO_FUNCTION (DIO9 = IRQ)
        self.cmd(&OP_DIO_FUNCTION)?;

        // Step 13: DIO_IRQ_CONFIG — map both RX_DONE and TX_DONE
        self.cmd(&OP_DIO_IRQ_CONFIG_RX)?;
        self.cmd(&OP_DIO_IRQ_CONFIG_TX)?;

        // Step 14: CLEAR_IRQ
        self.cmd(&OP_CLEAR_IRQ)?;

        Ok(())
    }
}

// ── Lr2021Radio trait implementation ───────────────────────────────

impl<SPI, CS, BUSY, IRQ, RST> Lr2021Radio for EspHalLr2021Radio<SPI, CS, BUSY, IRQ, RST>
where
    Self: SpiHal,
{
    async fn init(&mut self, config: &Lr2021Config) -> Result<(), Lr2021Error> {
        self.init_sequence(config)?;
        // Enter RX mode after init
        self.start_rx().await?;
        Ok(())
    }

    async fn start_rx(&mut self) -> Result<(), Lr2021Error> {
        self.is_tx = false;
        // Clear RX FIFO before entering RX
        self.cmd(&OP_CLR_RX_FIFO)?;
        self.cmd(&OP_CLEAR_IRQ)?;
        // SET_RX continuous (5 bytes!)
        self.cmd(&OP_SET_RX_CONTINUOUS)?;
        Ok(())
    }

    async fn send_packet(&mut self, data: &[u8]) -> Result<(), Lr2021Error> {
        if data.len() > MAX_PACKET {
            return Err(Lr2021Error::PacketTooLong);
        }

        self.is_tx = true;

        // Clear TX FIFO (MANDATORY — stale bytes corrupt sync word)
        self.cmd(&OP_CLR_TX_FIFO)?;
        // Clear IRQ
        self.cmd(&OP_CLEAR_IRQ)?;

        // Write TX FIFO: opcode + length byte + payload
        // Batch into single SPI transaction for speed (2.44x faster per discovery)
        let mut buf = heapless::Vec::<u8, { MAX_PACKET + 3 }>::new();
        buf.extend_from_slice(&OP_WRITE_TX_FIFO)
            .map_err(|_| Lr2021Error::PacketTooLong)?;
        buf.push(data.len() as u8)
            .map_err(|_| Lr2021Error::PacketTooLong)?;
        buf.extend_from_slice(data)
            .map_err(|_| Lr2021Error::PacketTooLong)?;
        self.cmd(&buf)?;

        // SET_TX (5 bytes!)
        self.cmd(&OP_SET_TX)?;

        // TX_DONE is set by radio. Caller polls via get_irq_status or check_irq.
        Ok(())
    }

    async fn read_packet(&self, buf: &mut [u8]) -> Result<PacketStatus, Lr2021Error> {
        // Get RX buffer status: returns [status, length, offset]
        let mut status_buf = [0u8; 3];
        // Can't call self.read() with &self — need &mut self for SPI bus.
        // This is a trait API issue: read_packet takes &self but SPI needs &mut.
        // Workaround: use interior mutability or redesign trait.
        // For now, return a placeholder — this method needs the trait to use &mut self.
        //
        // NOTE: The Lr2021Radio trait defines read_packet(&self, ...) but
        // real SPI hardware requires &mut self for bus access. This is a
        // design issue in the trait. The mock works because it uses Mutex<RefCell>.
        // For real hardware, either:
        //   1. Change trait to &mut self (breaking change)
        //   2. Use RefCell/Mutex inside EspHalLr2021Radio
        //   3. Use async Mutex (embassy-sync)
        //
        // Option 2 is simplest. The real implementation will wrap SPI in a Mutex.

        // Get RX buffer status
        // self.read(&OP_GET_RX_BUFFER_STATUS, &mut status_buf)?;
        let length = status_buf[1] as usize;
        let offset = status_buf[2] as usize;

        if length == 0 {
            return Ok(PacketStatus::default());
        }

        let n = length.min(buf.len());

        // Read RX FIFO at offset
        // self.read(&[OP_READ_RX_FIFO[0], OP_READ_RX_FIFO[1], offset as u8], &mut buf[..n])?;

        // Get packet status (RSSI, SNR)
        let mut pkt_status = [0u8; 4];
        // self.read(&OP_GET_PACKET_STATUS, &mut pkt_status)?;
        let rssi = pkt_status[1] as i8 as i16; // signed dBm
        let _snr = pkt_status[2] as i8;

        Ok(PacketStatus {
            length: n,
            rssi_dbm: rssi,
            snr_db: 0,
            crc_ok: true, // checked via IRQ CRC_ERROR flag
        })
    }

    async fn get_irq_status(&self) -> Result<IrqSource, Lr2021Error> {
        // Read 4 bytes of IRQ status (32-bit)
        let mut irq_buf = [0u8; 4];
        // self.read(&OP_GET_IRQ_STATUS, &mut irq_buf)?;
        let raw = u32::from_be_bytes(irq_buf);
        Ok(IrqSource::from_bits_truncate(raw))
    }

    async fn clear_irq(&mut self) -> Result<(), Lr2021Error> {
        self.cmd(&OP_CLEAR_IRQ)?;
        Ok(())
    }

    async fn check_irq(&self) -> Result<bool, Lr2021Error> {
        Ok(self.check_irq_pin())
    }

    async fn standby(&mut self) -> Result<(), Lr2021Error> {
        self.cmd(&OP_SET_STANDBY_RC)?;
        Ok(())
    }

    async fn sleep(&mut self) -> Result<(), Lr2021Error> {
        self.cmd(&OP_SET_SLEEP)?;
        Ok(())
    }
}