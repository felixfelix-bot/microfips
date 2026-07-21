//! Standalone host-side test harness for LR2021 framing and mock transport.
//!
//! Includes the LR2021 modules via #[path] to avoid pulling in esp-hal
//! and its portable-atomic dependency chain. This allows running unit tests
//! on the host (x86_64) without any embedded toolchain.

extern crate alloc;

#[path = "../../microfips-esp-transport/src/lr2021_framing.rs"]
pub mod lr2021_framing;

#[path = "../../microfips-esp-transport/src/lr2021_spi.rs"]
pub mod lr2021_spi;

#[path = "../../microfips-esp-transport/src/lr2021_transport.rs"]
pub mod lr2021_transport;

#[cfg(test)]
mod tests {
    use embassy_futures::block_on;
    use crate::lr2021_framing as framing;
    use crate::lr2021_spi as spi;

    // ═══════════════════════════════════════════════════════════════════
    // Framing tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_tx_small_chunk() {
        let mut f = framing::TxFramer::new();
        assert_eq!(f.push(b"hello"), 5);
        assert!(!f.is_full());
        assert_eq!(f.pending(), 5);
    }

    #[test]
    fn test_tx_fill_exactly_max() {
        let mut f = framing::TxFramer::new();
        let data = [0xABu8; framing::MAX_PACKET];
        let consumed = f.push(&data);
        assert_eq!(consumed, framing::MAX_PACKET);
        assert!(f.is_full());

        let mut pkt = [0u8; framing::MAX_PACKET];
        let n = f.take_packet(&mut pkt).unwrap();
        assert_eq!(n, framing::MAX_PACKET);
    }

    #[test]
    fn test_rx_push_and_drain() {
        let mut f = framing::RxFramer::new();
        f.push_packet(b"hello world");
        let mut buf = [0u8; 64];
        let n = f.drain(&mut buf);
        assert_eq!(n, 11);
        assert_eq!(&buf[..n], b"hello world");
    }

    #[test]
    fn test_roundtrip_small_frame() {
        let mut tx = framing::TxFramer::new();
        let mut rx = framing::RxFramer::new();

        let header = 20u16.to_le_bytes();
        let payload = [0x55u8; 20];

        tx.push(&header);
        tx.push(&payload);

        let mut pkt = [0u8; framing::MAX_PACKET];
        let n = tx.take_packet(&mut pkt).unwrap();

        rx.push_packet(&pkt[..n]);

        let mut buf = [0u8; 64];
        let n = rx.drain(&mut buf);
        assert_eq!(n, 22);
        assert_eq!(&buf[..2], &header);
        assert_eq!(&buf[2..22], &payload);
    }

    #[test]
    fn test_roundtrip_large_frame_fragments() {
        let mut tx = framing::TxFramer::new();
        let mut rx = framing::RxFramer::new();

        let payload = [0x77u8; 500];
        let header = 500u16.to_le_bytes();

        tx.push(&header);
        let consumed1 = tx.push(&payload);
        assert_eq!(consumed1, 253);
        assert!(tx.is_full());

        let mut pkt1 = [0u8; framing::MAX_PACKET];
        let pkt1_len = tx.take_packet(&mut pkt1).unwrap();
        rx.push_packet(&pkt1[..pkt1_len]);

        let consumed2 = tx.push(&payload[consumed1..]);
        assert_eq!(consumed2, 247);

        let mut pkt2 = [0u8; framing::MAX_PACKET];
        let pkt2_len = tx.take_packet(&mut pkt2).unwrap();
        rx.push_packet(&pkt2[..pkt2_len]);

        let mut buf = [0u8; 600];
        let total = rx.drain(&mut buf);
        assert_eq!(total, 502);
        assert_eq!(&buf[..2], &header);
        assert_eq!(buf[2..502], [0x77u8; 500]);
    }

    #[test]
    fn test_rx_multiple_packets_coalesced() {
        let mut f = framing::RxFramer::new();
        f.push_packet(b"AAA");
        f.push_packet(b"BBB");
        let mut buf = [0u8; 64];
        let n = f.drain(&mut buf);
        assert_eq!(n, 6);
        assert_eq!(&buf[..n], b"AAABBB");
    }

    // ═══════════════════════════════════════════════════════════════════
    // SPI driver config/IRQ tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_config_default_matches_baseline() {
        let c = spi::Lr2021Config::default();
        assert_eq!(c.freq_mhz, 2440.0);
        assert_eq!(c.bitrate_kbps, 2600);
        assert_eq!(c.tx_power_dbm, 12);
        assert_eq!(c.payload_length, 255);
        assert!(c.crc_enabled);
    }

    #[test]
    fn test_irq_flags() {
        use spi::IrqSource;
        let irq = IrqSource::TX_DONE | IrqSource::RX_DONE;
        assert!(irq.contains(IrqSource::TX_DONE));
        assert!(irq.contains(IrqSource::RX_DONE));
        assert!(!irq.contains(IrqSource::CRC_ERROR));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Mock radio tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_mock_radio_init_send_recv() {
        block_on(async {
            use spi::{Lr2021Config, Lr2021Radio, MockLr2021Radio};

            let mut radio = MockLr2021Radio::new();
            let config = Lr2021Config::default();

            radio.init(&config).await.unwrap();
            assert!(radio.is_initialized().await);

            // Load RX packet
            radio.load_rx_packet(b"test packet data").await;
            radio.start_rx().await.unwrap();

            // Read packet
            let mut buf = [0u8; 64];
            let status = radio.read_packet(&mut buf).await.unwrap();
            assert_eq!(status.length, 16);
            assert_eq!(&buf[..16], b"test packet data");

            // Send packet
            radio.send_packet(b"outgoing").await.unwrap();
            let tx_packets = radio.get_tx_packets().await;
            assert_eq!(tx_packets.len(), 1);
            assert_eq!(&tx_packets[0], b"outgoing");
        });
    }

    // ═══════════════════════════════════════════════════════════════════
    // Transport trait tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_transport_not_initialized_error() {
        block_on(async {
            use crate::lr2021_transport::{Lr2021Transport, TransportError};
            use microfips_protocol::transport::Transport;
            use spi::MockLr2021Radio;

            let radio = MockLr2021Radio::new();
            let mut transport = Lr2021Transport::new(radio);

            let result = transport.send(b"data").await;
            assert_eq!(result, Err(TransportError::NotInitialized));

            let mut buf = [0u8; 32];
            let result = transport.recv(&mut buf).await;
            assert_eq!(result, Err(TransportError::NotInitialized));
        });
    }

    #[test]
    fn test_transport_send_flush() {
        block_on(async {
            use crate::lr2021_transport::Lr2021Transport;
            use microfips_protocol::transport::Transport;
            use spi::{Lr2021Config, MockLr2021Radio};

            let radio = MockLr2021Radio::new();
            let mut transport = Lr2021Transport::new(radio);
            transport.init(&Lr2021Config::default()).await.unwrap();
            transport.wait_ready().await.unwrap();

            // Send data then flush
            transport.send(b"Hello LR2021!").await.unwrap();
            transport.flush_tx().await.unwrap();

            // Verify radio captured TX
            let tx_packets = transport.radio.get_tx_packets().await;
            assert!(!tx_packets.is_empty());
            let total: usize = tx_packets.iter().map(|p| p.len()).sum();
            assert_eq!(total, 13);
        });
    }

    #[test]
    fn test_transport_large_payload_fragmentation() {
        block_on(async {
            use crate::lr2021_transport::Lr2021Transport;
            use microfips_protocol::transport::Transport;
            use spi::{Lr2021Config, MockLr2021Radio};

            let radio = MockLr2021Radio::new();
            let mut transport = Lr2021Transport::new(radio);
            transport.init(&Lr2021Config::default()).await.unwrap();

            // 600 bytes > MAX_PACKET (255)
            let payload = [0xCDu8; 600];
            transport.send(&payload).await.unwrap();
            transport.flush_tx().await.unwrap();

            let tx_packets = transport.radio.get_tx_packets().await;
            assert!(tx_packets.len() >= 2, "should fragment");
            let total: usize = tx_packets.iter().map(|p| p.len()).sum();
            assert_eq!(total, 600);
        });
    }

    #[test]
    fn test_transport_recv_after_irq() {
        block_on(async {
            use crate::lr2021_transport::Lr2021Transport;
            use microfips_protocol::transport::Transport;
            use spi::{Lr2021Config, MockLr2021Radio};

            let radio = MockLr2021Radio::new();
            let mut transport = Lr2021Transport::new(radio);
            transport.init(&Lr2021Config::default()).await.unwrap();

            // Simulate radio receiving a packet
            transport.radio.load_rx_packet(b"incoming data!").await;
            transport.handle_irq().await.unwrap();

            // Recv should return the data
            let mut buf = [0u8; 64];
            let n = transport.recv(&mut buf).await.unwrap();
            assert_eq!(n, 14);
            assert_eq!(&buf[..n], b"incoming data!");
        });
    }

    // ═══════════════════════════════════════════════════════════════════
    // Full FrameWriter → LR2021 Transport → FrameReader roundtrip
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_framewriter_through_lr2021_to_framereader() {
        block_on(async {
            use crate::lr2021_transport::Lr2021Transport;
            use microfips_protocol::transport::{FrameReader, FrameWriter, Transport};
            use spi::{Lr2021Config, MockLr2021Radio};

            // TX side
            let tx_radio = MockLr2021Radio::new();
            let mut tx_transport = Lr2021Transport::new(tx_radio);
            tx_transport.init(&Lr2021Config::default()).await.unwrap();

            let payload = b"FIPS mesh over LR2021 LoRa - encrypted!";
            let mut fw = FrameWriter::new(tx_transport);
            fw.send_frame(payload).await.unwrap();

            let mut tx_transport = fw.into_inner();
            tx_transport.flush_tx().await.unwrap();

            let tx_packets = tx_transport.radio.get_tx_packets().await;
            assert!(!tx_packets.is_empty());

            // RX side
            let rx_radio = MockLr2021Radio::new();
            let mut rx_transport = Lr2021Transport::new(rx_radio);
            rx_transport.init(&Lr2021Config::default()).await.unwrap();

            for pkt in &tx_packets {
                rx_transport.radio.load_rx_packet(pkt).await;
                rx_transport.handle_irq().await.unwrap();
            }

            let mut fr = FrameReader::new(rx_transport);
            let mut out = [0u8; 256];
            let n = fr.recv_frame(&mut out, 5000).await.unwrap();
            assert_eq!(n, payload.len());
            assert_eq!(&out[..n], payload);
        });
    }

    #[test]
    fn test_fips_handshake_msg1_fits_single_packet() {
        // Noise IK MSG1 is ~114 bytes wire — should fit in a single FLRC packet
        block_on(async {
            use crate::lr2021_transport::Lr2021Transport;
            use microfips_protocol::transport::{FrameWriter, Transport};
            use spi::{Lr2021Config, MockLr2021Radio};

            let tx_radio = MockLr2021Radio::new();
            let mut tx_transport = Lr2021Transport::new(tx_radio);
            tx_transport.init(&Lr2021Config::default()).await.unwrap();

            // Simulate MSG1: 2-byte frame header + 114-byte Noise IK payload
            let fake_msg1 = [0xAAu8; 114];
            let mut fw = FrameWriter::new(tx_transport);
            fw.send_frame(&fake_msg1).await.unwrap();

            let mut tx_transport = fw.into_inner();
            tx_transport.flush_tx().await.unwrap();

            let tx_packets = tx_transport.radio.get_tx_packets().await;
            // 114 + 2 header = 116 bytes < 255 → single packet
            assert_eq!(tx_packets.len(), 1, "MSG1 should fit in one FLRC packet");
            assert_eq!(tx_packets[0].len(), 116);
        });
    }
}
