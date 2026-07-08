//! Peer table module for ESP-NOW routing
//!
//! This module provides the core peer table functionality for mapping
//! FIPS node addresses to ESP-NOW MAC addresses.

use heapless::Vec;

use microfips_core::NodeAddr;
use microfips_esp_transport::MacAddress;

/// Maximum number of peers in the routing table
pub const MAX_PEERS: usize = 20;

/// A peer entry in the routing table
#[derive(Debug, Clone, PartialEq)]
pub struct PeerEntry {
    /// FIPS node address (16 bytes, SHA-256 of public key)
    pub node_addr: NodeAddr,
    /// ESP-NOW MAC address (6 bytes)
    pub mac_addr: MacAddress,
    /// Last seen timestamp for LRU eviction
    pub last_seen: embassy_time::Instant,
    /// Peer status
    pub status: PeerStatus,
}

/// Peer status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    /// Peer is active and reachable
    Active,
    /// Peer is suspected to be down
    Suspect,
    /// Peer is confirmed to be down
    Dead,
}

/// Error types for peer table operations
#[derive(Debug)]
pub enum PeerTableError {
    /// Peer not found in routing table
    PeerNotFound,
    /// Routing table is full
    TableFull,
    /// Invalid node address format
    InvalidNodeAddr,
}

/// Result type for peer table operations
pub type PeerTableResult<T> = Result<T, PeerTableError>;

/// Peer table for mapping FIPS node addresses to ESP-NOW MAC addresses
pub struct PeerTable {
    /// Peer entries
    peers: Vec<PeerEntry, MAX_PEERS>,
}

impl PeerTable {
    /// Create a new empty peer table
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),
        }
    }

    /// Add or update a peer in the peer table
    pub fn add_or_update_peer(&mut self, node_addr: NodeAddr, mac_addr: MacAddress) -> PeerTableResult<()> {
        // Check if peer already exists
        if let Some(pos) = self.peers.iter().position(|p| p.node_addr == node_addr) {
            // Update existing peer
            let peer = &mut self.peers[pos];
            peer.mac_addr = mac_addr;
            peer.last_seen = embassy_time::Instant::now();
            peer.status = PeerStatus::Active;
            return Ok(());
        }

        // Add new peer if space available
        self.peers
            .push(PeerEntry {
                node_addr,
                mac_addr,
                last_seen: embassy_time::Instant::now(),
                status: PeerStatus::Active,
            })
            .map_err(|_| PeerTableError::TableFull)?;

        Ok(())
    }

    /// Remove a peer from the peer table
    pub fn remove_peer(&mut self, node_addr: &NodeAddr) -> PeerTableResult<()> {
        let pos = self
            .peers
            .iter()
            .position(|p| p.node_addr == *node_addr)
            .ok_or(PeerTableError::PeerNotFound)?;
        
        self.peers.remove(pos);
        Ok(())
    }

    /// Get peer entry by node address
    pub fn get_peer(&self, node_addr: &NodeAddr) -> PeerTableResult<&PeerEntry> {
        self.peers
            .iter()
            .find(|p| p.node_addr == *node_addr)
            .ok_or(PeerTableError::PeerNotFound)
    }

    /// Get MAC address for a node address
    pub fn get_mac_addr(&self, node_addr: &NodeAddr) -> PeerTableResult<MacAddress> {
        self.get_peer(node_addr).map(|p| p.mac_addr)
    }

    /// Get all peers in the peer table
    pub fn get_all_peers(&self) -> &[PeerEntry] {
        &self.peers
    }

    /// Get the number of peers in the table
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if the peer table is empty
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Check if the peer table is full
    pub fn is_full(&self) -> bool {
        self.peers.len() >= MAX_PEERS
    }

    /// Evict stale peers using LRU (Least Recently Used) algorithm
    pub fn evict_stale_peers(&mut self) {
        if self.peers.len() < MAX_PEERS {
            return;
        }

        // Sort by last_seen (oldest first) and remove the oldest
        self.peers.sort_by_key(|p| p.last_seen);
        self.peers.drain(0..self.peers.len() - MAX_PEERS + 1);
    }

    /// Update peer status
    pub fn update_peer_status(&mut self, node_addr: &NodeAddr, status: PeerStatus) -> PeerTableResult<()> {
        let peer = self
            .peers
            .iter_mut()
            .find(|p| p.node_addr == *node_addr)
            .ok_or(PeerTableError::PeerNotFound)?;
        
        peer.status = status;
        Ok(())
    }

    /// Mark peer as active (updates last_seen time)
    pub fn mark_peer_active(&mut self, node_addr: &NodeAddr) -> PeerTableResult<()> {
        let peer = self
            .peers
            .iter_mut()
            .find(|p| p.node_addr == *node_addr)
            .ok_or(PeerTableError::PeerNotFound)?;
        
        peer.last_seen = embassy_time::Instant::now();
        peer.status = PeerStatus::Active;
        Ok(())
    }

    /// Find peers by status
    pub fn find_peers_by_status(&self, status: PeerStatus) -> impl Iterator<Item = &PeerEntry> {
        self.peers.iter().filter(move |p| p.status == status)
    }

    /// Get active peers only
    pub fn get_active_peers(&self) -> impl Iterator<Item = &PeerEntry> {
        self.find_peers_by_status(PeerStatus::Active)
    }

    /// Get peer statistics
    pub fn get_stats(&self) -> PeerTableStats {
        let total = self.peers.len();
        let active = self.find_peers_by_status(PeerStatus::Active).count();
        let suspect = self.find_peers_by_status(PeerStatus::Suspect).count();
        let dead = self.find_peers_by_status(PeerStatus::Dead).count();

        PeerTableStats {
            total,
            active,
            suspect,
            dead,
            capacity: MAX_PEERS,
        }
    }
}

/// Peer table statistics
#[derive(Debug, Clone, Copy)]
pub struct PeerTableStats {
    /// Total number of peers
    pub total: usize,
    /// Number of active peers
    pub active: usize,
    /// Number of suspect peers
    pub suspect: usize,
    /// Number of dead peers
    pub dead: usize,
    /// Maximum capacity
    pub capacity: usize,
}

impl Default for PeerTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_peer_table() {
        let table = PeerTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn test_add_peer() {
        let mut table = PeerTable::new();
        let node_addr = [1u8; 16];
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        assert!(table.add_or_update_peer(node_addr, mac_addr).is_ok());
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());
    }

    #[test]
    fn test_get_peer() {
        let mut table = PeerTable::new();
        let node_addr = [1u8; 16];
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        table.add_or_update_peer(node_addr, mac_addr).unwrap();
        let peer = table.get_peer(&node_addr).unwrap();
        assert_eq!(peer.node_addr, node_addr);
        assert_eq!(peer.mac_addr, mac_addr);
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn test_remove_peer() {
        let mut table = PeerTable::new();
        let node_addr = [1u8; 16];
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        table.add_or_update_peer(node_addr, mac_addr).unwrap();
        assert_eq!(table.len(), 1);
        
        table.remove_peer(&node_addr).unwrap();
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
    }

    #[test]
    fn test_update_peer() {
        let mut table = PeerTable::new();
        let node_addr = [1u8; 16];
        let mac_addr1 = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let mac_addr2 = MacAddress([0xBB, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        table.add_or_update_peer(node_addr, mac_addr1).unwrap();
        let peer = table.get_peer(&node_addr).unwrap();
        assert_eq!(peer.mac_addr, mac_addr1);
        
        table.add_or_update_peer(node_addr, mac_addr2).unwrap();
        let peer = table.get_peer(&node_addr).unwrap();
        assert_eq!(peer.mac_addr, mac_addr2);
        assert_eq!(table.len(), 1); // Should still be 1, not 2
    }

    #[test]
    fn test_peer_status() {
        let mut table = PeerTable::new();
        let node_addr = [1u8; 16];
        let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        
        table.add_or_update_peer(node_addr, mac_addr).unwrap();
        
        table.update_peer_status(&node_addr, PeerStatus::Suspect).unwrap();
        let peer = table.get_peer(&node_addr).unwrap();
        assert_eq!(peer.status, PeerStatus::Suspect);
        
        table.mark_peer_active(&node_addr).unwrap();
        let peer = table.get_peer(&node_addr).unwrap();
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn test_peer_stats() {
        let mut table = PeerTable::new();
        
        // Add some peers
        for i in 0..5 {
            let node_addr = [i as u8; 16];
            let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, i as u8]);
            table.add_or_update_peer(node_addr, mac_addr).unwrap();
        }
        
        // Update some statuses
        let node_addr1 = [1u8; 16];
        let node_addr2 = [2u8; 16];
        table.update_peer_status(&node_addr1, PeerStatus::Suspect).unwrap();
        table.update_peer_status(&node_addr2, PeerStatus::Dead).unwrap();
        
        let stats = table.get_stats();
        assert_eq!(stats.total, 5);
        assert_eq!(stats.active, 3);
        assert_eq!(stats.suspect, 1);
        assert_eq!(stats.dead, 1);
        assert_eq!(stats.capacity, MAX_PEERS);
    }
}