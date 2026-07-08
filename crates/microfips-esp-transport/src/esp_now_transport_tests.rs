//! Comprehensive tests for ESP-NOW transport implementation
//!
//! Tests cover:
//! - MacAddress struct functionality
//! - Error types
//! - Transport trait behavior (with mocks)
//! - Send/receive logic
//! - Peer management

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Debug;
    use embassy_futures::yield_now;
    use embassy_time::{Duration, Timer};

    // ── Mock Transport for testing ─────────────────────────────────────────────

    #[derive(Debug)]
    pub struct MockEspNowTransport {
        peers: heapless::Vec<MacAddress, 8>,
        sent_data: heapless::Vec<u8, 256>,
        recv_queue: heapless::Vec<u8, 256>,
        error: Option<EspNowError>,
    }

    impl MockEspNowTransport {
        pub fn new() -> Self {
            Self {
                peers: heapless::Vec::new(),
                sent_data: heapless::Vec::new(),
                recv_queue: heapless::Vec::new(),
                error: None,
            }
        }

        pub fn with_error(mut self, error: EspNowError) -> Self {
            self.error = Some(error);
            self
        }

        pub fn add_recv_data(&mut self, data: &[u8]) {
            self.recv_queue.extend_from_slice(data).ok();
        }

        pub fn get_sent_data(&self) -> &[u8] {
            &self.sent_data
        }

        pub fn get_peers(&self) -> &[MacAddress] {
            &self.peers
        }
    }

    #[async_trait::async_trait]
    impl Transport for MockEspNowTransport {
        type Error = EspNowError;

        async fn wait_ready(&mut self) -> Result<(), Self::Error> {
            if let Some(error) = self.error {
                return Err(error);
            }
            Ok(())
        }

        async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
            if let Some(error) = self.error {
                return Err(error);
            }

            if data.len() > ESP_NOW_PAYLOAD_MAX {
                return Err(EspNowError::SendFailed);
            }

            self.sent_data.clear();
            self.sent_data.extend_from_slice(data).map_err(|_| EspNowError::SendFailed)?;
            Ok(())
        }

        async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            if let Some(error) = self.error {
                return Err(error);
            }

            if self.recv_queue.is_empty() {
                return Err(EspNowError::Timeout);
            }

            let len = self.recv_queue.len().min(buf.len());
            buf[..len].copy_from_slice(&self.recv_queue[..len]);
            self.recv_queue.clear();
            Ok(len)
        }
    }

    // ── MacAddress Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_mac_address_from_bytes_valid() {
        let valid_mac = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mac = MacAddress::from_bytes(&valid_mac);
        assert!(mac.is_some());
        assert_eq!(mac.unwrap().0, valid_mac);
    }

    #[test]
    fn test_mac_address_from_bytes_too_short() {
        let short_mac = [0x12, 0x34, 0x56];
        let mac = MacAddress::from_bytes(&short_mac);
        assert!(mac.is_none());
    }

    #[test]
    fn test_mac_address_broadcast() {
        assert!(MacAddress::BROADCAST.is_broadcast());
        assert!(!MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]).is_broadcast());
    }

    #[test]
    fn test_mac_address_display() {
        let mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let display = format!("{}", mac);
        assert_eq!(display, "12:34:56:78:9a:bc");
    }

    #[test]
    fn test_mac_address_as_ptr() {
        let mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let ptr = mac.as_ptr();
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!(*ptr, 0x12);
        }
    }

    // ── EspNowError Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_esp_now_error_debug() {
        let errors = [
            EspNowError::InitFailed,
            EspNowError::SendFailed,
            EspNowError::PeerNotFound,
            EspNowError::NoPeer,
            EspNowError::Timeout,
            EspNowError::NotInitialized,
        ];

        for error in errors {
            let debug_str = format!("{:?}", error);
            assert!(!debug_str.is_empty());
        }
    }

    #[test]
    fn test_esp_now_error_clone_copy() {
        let error = EspNowError::SendFailed;
        let cloned = error;
        let copied = error;
        assert_eq!(std::mem::discriminant(&error), std::mem::discriminant(&cloned));
        assert_eq!(std::mem::discriminant(&error), std::mem::discriminant(&copied));
    }

    // ── Mock Transport Tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_mock_transport_wait_ready_success() {
        let mut transport = MockEspNowTransport::new();
        assert!(transport.wait_ready().await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_transport_wait_ready_error() {
        let mut transport = MockEspNowTransport::new().with_error(EspNowError::InitFailed);
        let result = transport.wait_ready().await;
        assert!(matches!(result, Err(EspNowError::InitFailed)));
    }

    #[tokio::test]
    async fn test_mock_transport_send_success() {
        let mut transport = MockEspNowTransport::new();
        let data = b"hello world";
        
        assert!(transport.send(data).await.is_ok());
        assert_eq!(transport.get_sent_data(), data);
    }

    #[tokio::test]
    async fn test_mock_transport_send_too_large() {
        let mut transport = MockEspNowTransport::new();
        let large_data = [0u8; ESP_NOW_PAYLOAD_MAX + 1];
        
        let result = transport.send(&large_data).await;
        assert!(matches!(result, Err(EspNowError::SendFailed)));
    }

    #[tokio::test]
    async fn test_mock_transport_send_error() {
        let mut transport = MockEspNowTransport::new().with_error(EspNowError::SendFailed);
        let data = b"test";
        
        let result = transport.send(data).await;
        assert!(matches!(result, Err(EspNowError::SendFailed)));
    }

    #[tokio::test]
    async fn test_mock_transport_recv_success() {
        let mut transport = MockEspNowTransport::new();
        let test_data = b"received data";
        transport.add_recv_data(test_data);
        
        let mut buf = [0u8; 32];
        let len = transport.recv(&mut buf).await.unwrap();
        
        assert_eq!(len, test_data.len());
        assert_eq!(&buf[..len], test_data);
    }

    #[tokio::test]
    async fn test_mock_transport_recv_timeout() {
        let mut transport = MockEspNowTransport::new();
        let mut buf = [0u8; 32];
        
        let result = transport.recv(&mut buf).await;
        assert!(matches!(result, Err(EspNowError::Timeout)));
    }

    #[tokio::test]
    async fn test_mock_transport_recv_error() {
        let mut transport = MockEspNowTransport::new().with_error(EspNowError::NotInitialized);
        let mut buf = [0u8; 32];
        
        let result = transport.recv(&mut buf).await;
        assert!(matches!(result, Err(EspNowError::NotInitialized)));
    }

    // ── Frame Integration Tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_frame_writer_esp_now_transport() {
        let mut transport = MockEspNowTransport::new();
        let mut frame_writer = FrameWriter::new(transport);
        
        let payload = b"test frame payload";
        assert!(frame_writer.send_frame(payload).await.is_ok());
        
        let transport = frame_writer.into_inner();
        let sent_data = transport.get_sent_data();
        
        // Should have [length: u16le][payload]
        assert_eq!(sent_data.len(), 2 + payload.len());
        let len = u16::from_le_bytes([sent_data[0], sent_data[1]]);
        assert_eq!(len, payload.len());
        assert_eq!(&sent_data[2..], payload);
    }

    #[tokio::test]
    async fn test_frame_reader_esp_now_transport() {
        let mut transport = MockEspNowTransport::new();
        let mut frame_reader = FrameReader::new(transport);
        
        // Send frame data to transport
        let payload = b"frame test data";
        let mut frame_data = Vec::new();
        frame_data.extend_from_slice(&(payload.len() as u16).to_le_bytes().as_ref());
        frame_data.extend_from_slice(payload);
        
        let mut transport = MockEspNowTransport::new();
        transport.add_recv_data(&frame_data);
        let mut frame_reader = FrameReader::new(transport);
        
        let mut out_buf = [0u8; 64];
        let len = frame_reader.recv_frame(&mut out_buf, 1000).await.unwrap();
        
        assert_eq!(len, payload.len());
        assert_eq!(&out_buf[..len], payload);
    }

    // ── Stress Tests ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_sends_receives() {
        let mut transport = MockEspNowTransport::new();
        
        for i in 0..10 {
            let data = [i as u8; 32];
            assert!(transport.send(&data).await.is_ok());
            
            let sent = transport.get_sent_data();
            assert_eq!(sent, &data);
            
            // Add data for recv
            transport.add_recv_data(&data);
            let mut buf = [0u8; 32];
            let len = transport.recv(&mut buf).await.unwrap();
            assert_eq!(len, data.len());
            assert_eq!(&buf[..len], &data);
        }
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        use futures::join;
        
        let mut transport1 = MockEspNowTransport::new();
        let mut transport2 = MockEspNowTransport::new();
        
        let send_task = async {
            transport1.send(b"concurrent test").await
        };
        
        let recv_task = async {
            let mut buf = [0u8; 32];
            transport2.recv(&mut buf).await
        };
        
        let (send_result, recv_result) = join!(send_task, recv_task);
        
        assert!(send_result.is_ok());
        // recv_result may be timeout since no data was added, which is expected
    }

    // ── Property-Based Tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_random_data_sizes() {
        use rand::Rng;
        
        let mut rng = rand::thread_rng();
        let mut transport = MockEspNowTransport::new();
        
        for _ in 0..20 {
            let size = rng.gen_range(1..=ESP_NOW_PAYLOAD_MAX);
            let data: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
            
            // Should succeed for valid sizes
            assert!(transport.send(&data).await.is_ok());
            
            // Clear sent data for next iteration
            let _ = transport.send(&[]).await; // This will clear the buffer
        }
    }

    #[tokio::test]
    async fn test_boundary_conditions() {
        let mut transport = MockEspNowTransport::new();
        
        // Test empty send
        assert!(transport.send(&[]).await.is_ok());
        
        // Test maximum size
        let max_data = [0u8; ESP_NOW_PAYLOAD_MAX];
        assert!(transport.send(&max_data).await.is_ok());
        
        // Test one byte over maximum (should fail)
        let too_large_data = [0u8; ESP_NOW_PAYLOAD_MAX + 1];
        assert!(matches!(transport.send(&too_large_data).await, Err(EspNowError::SendFailed)));
    }

    // ── Error Propagation Tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_error_propagation() {
        let mut transport = MockEspNowTransport::new().with_error(EspNowError::InitFailed);
        
        // All operations should fail with the same error
        assert!(matches!(transport.wait_ready().await, Err(EspNowError::InitFailed)));
        assert!(matches!(transport.send(&[]).await, Err(EspNowError::InitFailed)));
        assert!(matches!(transport.recv(&mut []).await, Err(EspNowError::InitFailed)));
    }

    #[tokio::test]
    async fn test_error_after_success() {
        let mut transport = MockEspNowTransport::new();
        
        // First operation succeeds
        assert!(transport.send(&[1, 2, 3]).await.is_ok());
        
        // Set error state
        transport = MockEspNowTransport::new().with_error(EspNowError::SendFailed);
        
        // Subsequent operations should fail
        assert!(matches!(transport.send(&[]).await, Err(EspNowError::SendFailed)));
    }
}