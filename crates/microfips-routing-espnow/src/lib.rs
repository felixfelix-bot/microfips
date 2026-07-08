//! ESP-NOW routing layer for microFIPS mesh with MAC-to-node-address mapping
//!
//! This crate implements P2.1: MAC ↔ FIPS node address mapping for the ESP-NOW transport.
//! It provides:
//! - Peer table mapping 16-byte FIPS node addresses to 6-byte MAC addresses
//! - FIPS routing resolution (node_addr → MAC → ESP-NOW send)
//! - Broadcast discovery for finding peers
//! - Dynamic peer management

pub mod peer_table;

use core::array::TryFromSliceError;

use microfips_core::NodeAddr;
use microfips_esp_transport::{EspNowTransport, MacAddress, EspNowError};
use microfips_protocol::transport::Transport;

pub use peer_table::{PeerTable, PeerEntry, PeerStatus, PeerTableError, PeerTableStats, MAX_PEERS};

/// Error types for routing operations
#[derive(Debug)]
pub enum RoutingError {
    /// Peer not found in routing table
    PeerNotFound,
    /// Routing table is full
    TableFull,
    /// Invalid node address format
    InvalidNodeAddr,
    /// Transport error
    TransportError(EspNowError),
    /// Peer table error
    PeerTableError(peer_table::PeerTableError),
}

impl From<EspNowError> for RoutingError {
    fn from(err: EspNowError) -> Self {
        RoutingError::TransportError(err)
    }
}

impl From<peer_table::PeerTableError> for RoutingError {
    fn from(err: peer_table::PeerTableError) -> Self {
        RoutingError::PeerTableError(err)
    }
}

impl From<TryFromSliceError> for RoutingError {
    fn from(_: TryFromSliceError) -> Self {
        RoutingError::InvalidNodeAddr
    }
}

/// Result type for routing operations
pub type RoutingResult<T> = Result<T, RoutingError>;

/// ESP-NOW router that combines peer table management with ESP-NOW transport
pub struct EspNowRouter {
    /// Peer table for mapping node addresses to MAC addresses
    peer_table: PeerTable,
    /// ESP-NOW transport for sending messages
    transport: EspNowTransport,
}

impl EspNowRouter {
    /// Create a new ESP-NOW router with the given ESP-NOW transport
    pub fn new(transport: EspNowTransport) -> Self {
        Self {
            peer_table: PeerTable::new(),
            transport,
        }
    }

    /// Add or update a peer in the routing table
    pub fn add_or_update_peer(&mut self, node_addr: NodeAddr, mac_addr: MacAddress) -> RoutingResult<()> {
        self.peer_table.add_or_update_peer(node_addr, mac_addr)
    }

    /// Remove a peer from the routing table
    pub fn remove_peer(&mut self, node_addr: &NodeAddr) -> RoutingResult<()> {
        self.peer_table.remove_peer(node_addr)
    }

    /// Get peer entry by node address
    pub fn get_peer(&self, node_addr: &NodeAddr) -> RoutingResult<&PeerEntry> {
        self.peer_table.get_peer(node_addr)
    }

    /// Get MAC address for a node address
    pub fn get_mac_addr(&self, node_addr: &NodeAddr) -> RoutingResult<MacAddress> {
        self.peer_table.get_mac_addr(node_addr)
    }

    /// Get all peers in the routing table
    pub fn get_peers(&self) -> &[PeerEntry] {
        self.peer_table.get_all_peers()
    }

    /// Get peer table statistics
    pub fn get_stats(&self) -> PeerTableStats {
        self.peer_table.get_stats()
    }

    /// Send a message to a node address (resolves MAC and sends via ESP-NOW)
    pub async fn send_to_node(&mut self, node_addr: &NodeAddr, data: &[u8]) -> RoutingResult<()> {
        let mac_addr = self.get_mac_addr(node_addr)?;
        
        // Update peer last seen time
        let _ = self.peer_table.mark_peer_active(node_addr);

        // Send via ESP-NOW transport
        self.transport.send_to(mac_addr, data).await?;
        Ok(())
    }

    /// Broadcast a discovery message to all ESP-NOW peers
    pub async fn broadcast_discovery(&mut self, data: &[u8]) -> RoutingResult<()> {
        // Use ESP-NOW broadcast MAC address (FF:FF:FF:FF:FF:FF)
        self.transport.broadcast(data).await?;
        Ok(())
    }

    /// Evict stale peers (LRU eviction to maintain MAX_PEERS limit)
    pub fn evict_stale_peers(&mut self) {
        self.peer_table.evict_stale_peers();
    }

    /// Update peer status (e.g., mark as suspect if no response)
    pub fn update_peer_status(&mut self, node_addr: &NodeAddr, status: PeerStatus) -> RoutingResult<()> {
        self.peer_table.update_peer_status(node_addr, status)
    }

    /// Get a reference to the underlying peer table
    pub fn peer_table(&self) -> &PeerTable {
        &self.peer_table
    }

    /// Get a mutable reference to the underlying peer table
    pub fn peer_table_mut(&mut self) -> &mut PeerTable {
        &mut self.peer_table
    }

    /// Get a reference to the underlying transport
    pub fn transport(&self) -> &EspNowTransport {
        &self.transport
    }

    /// Get a mutable reference to the underlying transport
    pub fn transport_mut(&mut self) -> &mut EspNowTransport {
        &mut self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embassy_time::Duration;

    // Mock EspNowTransport for testing
    struct MockEspNowTransport;
    
    impl MockEspNowTransport {
        async fn send_to(&self, _mac: MacAddress, _data: &[u8]) -> Result<(), EspNowError> {
            Ok(())
        }
        
        async fn broadcast(&self, _data: &[u8]) -> Result<(), EspNowError> {
            Ok(())
        }
    }

    #[test]
    fn test_peer_addition() {
        let transport = EspNowTransport::init().unwrap().0; // We'll just unwrap for tests
        let mut router = EspNowRouter::new(transport);
        
        let node_addr = NodeAddr([1u8; 16]);
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        assert!(router.add_or_update_peer(node_addr, mac_addr).is_ok());
        assert_eq!(router.get_peers().len(), 1);
    }

    #[test]
    fn test_mac_resolution() {
        let transport = EspNowTransport::init().unwrap().0;
        let mut router = EspNowRouter::new(transport);
        
        let node_addr = NodeAddr([1u8; 16]);
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        router.add_or_update_peer(node_addr, mac_addr).unwrap();
        let resolved_mac = router.get_mac_addr(&node_addr).unwrap();
        assert_eq!(resolved_mac, mac_addr);
    }

    #[test]
    fn test_peer_table_integration() {
        let transport = EspNowTransport::init().unwrap().0;
        let mut router = EspNowRouter::new(transport);
        
        let node_addr1 = NodeAddr([1u8; 16]);
        let node_addr2 = NodeAddr([2u8; 16]);
        let mac_addr1 = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let mac_addr2 = MacAddress([0xBB, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        // Add peers
        router.add_or_update_peer(node_addr1, mac_addr1).unwrap();
        router.add_or_update_peer(node_addr2, mac_addr2).unwrap();
        
        // Check stats
        let stats = router.get_stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 2);
        
        // Update peer status
        router.update_peer_status(&node_addr1, PeerStatus::Suspect).unwrap();
        let stats = router.get_stats();
        assert_eq!(stats.suspect, 1);
        assert_eq!(stats.active, 1);
        
        // Remove peer
        router.remove_peer(&node_addr2).unwrap();
        let stats = router.get_stats();
        assert_eq!(stats.total, 1);
    }
}