# microfips-routing-espnow

ESP-NOW routing layer for microFIPS mesh with MAC-to-node-address mapping.

This crate implements P2.1: MAC ↔ FIPS node address mapping for the ESP-NOW transport.

## Overview

This crate provides:

- **Peer Table**: Mapping 16-byte FIPS node addresses to 6-byte ESP-NOW MAC addresses
- **FIPS Routing Resolution**: `node_addr → MAC → ESP-NOW send`
- **Broadcast Discovery**: ESP-NOW broadcast with FIPS identity for peer discovery
- **Dynamic Peer Management**: LRU eviction, peer status tracking, statistics

## Architecture

```
FIPS Node A ←→ [ESP-NOW unicast] ←→ FIPS Node B
              (peer-to-peer, no IP, no hierarchy)
```

### Key Components

#### `PeerTable`
- Maps `NodeAddr` (16 bytes) to `MacAddress` (6 bytes)
- LRU eviction for maximum 20 peers (ESP-NOW limit)
- Peer status tracking (Active, Suspect, Dead)

#### `EspNowRouter`  
- Combines `PeerTable` with `EspNowTransport`
- High-level API for routing operations:
  - `send_to_node()`: Send to FIPS node address
  - `broadcast_discovery()`: Broadcast to all peers
  - `add_or_update_peer()`: Maintain peer table

## Usage

```rust
use microfips_routing_espnow::{EspNowRouter, NodeAddr, MacAddress};

// Initialize ESP-NOW transport
let (transport, _local_mac) = EspNowTransport::init().unwrap();

// Create router
let mut router = EspNowRouter::new(transport);

// Add peers
let node_addr = NodeAddr([1u8; 16]);
let mac_addr = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
router.add_or_update_peer(node_addr, mac_addr)?;

// Send to peer by node address
let data = b"Hello FIPS mesh!";
router.send_to_node(&node_addr, data).await?;

// Broadcast discovery
router.broadcast_discovery(b"DISCOVERY").await?;
```

## Peer Table Management

```rust
// Get peer information
if let Ok(peer) = router.get_peer(&node_addr) {
    println!("Peer MAC: {}", peer.mac_addr);
    println!("Status: {:?}", peer.status);
}

// Update peer status
router.update_peer_status(&node_addr, PeerStatus::Suspect)?;

// Get statistics
let stats = router.get_stats();
println!("Total peers: {}", stats.total);
println!("Active peers: {}", stats.active);
```

## Dependencies

- `microfips-core`: FIPS protocol primitives and `NodeAddr`
- `microfips-esp-transport`: ESP-NOW transport implementation
- `microfips-protocol`: Transport trait definitions
- `heapless`: No-std collections for embedded

## Features

- `log`: Enable logging support
- `esp32c3`: ESP32-C3 specific optimizations

## Testing

Run tests with:
```bash
cargo test -p microfips-routing-espnow
```

## Integration with FIPS Mesh

This crate implements the MAC-to-node-address mapping layer for the ESP-NOW FIPS mesh:

1. **FIPS routing decisions** target `NodeAddr` (16-byte FIPS addresses)
2. **ESP-NOW transport** requires `MacAddress` (6-byte WiFi MAC) 
3. **This crate** bridges the gap: `NodeAddr ↔ MacAddress`

The routing layer maintains a dynamic peer table that:
- Is populated via ESP-NOW broadcast discovery
- Is updated when peers are confirmed/reachable
- Evicts stale peers using LRU algorithm
- Supports mesh routing protocols (STP, bloom filters)

## Error Handling

The crate provides comprehensive error handling:

```rust
pub enum RoutingError {
    PeerNotFound,
    TableFull,
    InvalidNodeAddr,
    TransportError(EspNowError),
    PeerTableError(PeerTableError),
}
```

All operations return `RoutingResult<T>` for error propagation.

## License

MIT License - see LICENSE file in repository root.