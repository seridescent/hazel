use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::bail;

/// Port allocator for deployments. Reuses released ports before allocating new ones.
pub struct PortAllocator {
    port_min: u16,
    port_max: u16,
    next_new_port: u16,
    reclaimed: BinaryHeap<Reverse<u16>>,
}

impl PortAllocator {
    pub fn new(port_min: u16, port_max: u16) -> Self {
        Self {
            port_min,
            port_max,
            next_new_port: port_min,
            reclaimed: BinaryHeap::new(),
        }
    }

    pub fn allocate(&mut self) -> anyhow::Result<u16> {
        // Prefer reclaimed ports (lowest first)
        if let Some(Reverse(port)) = self.reclaimed.pop() {
            return Ok(port);
        }

        if self.next_new_port > self.port_max {
            bail!(
                "port range exhausted ({}-{})",
                self.port_min,
                self.port_max
            );
        }

        let port = self.next_new_port;
        self.next_new_port += 1;
        Ok(port)
    }

    pub fn release(&mut self, port: u16) {
        self.reclaimed.push(Reverse(port));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_from_range() {
        let mut alloc = PortAllocator::new(8000, 8002);

        assert_eq!(alloc.allocate().unwrap(), 8000);
        assert_eq!(alloc.allocate().unwrap(), 8001);
        assert_eq!(alloc.allocate().unwrap(), 8002);
    }

    #[test]
    fn exhaustion_returns_error() {
        let mut alloc = PortAllocator::new(8000, 8001);

        assert!(alloc.allocate().is_ok());
        assert!(alloc.allocate().is_ok());
        assert!(alloc.allocate().is_err());
    }

    #[test]
    fn reclaimed_ports_reused_first() {
        let mut alloc = PortAllocator::new(8000, 8010);

        let p1 = alloc.allocate().unwrap();
        let p2 = alloc.allocate().unwrap();
        let p3 = alloc.allocate().unwrap();

        assert_eq!(p1, 8000);
        assert_eq!(p2, 8001);
        assert_eq!(p3, 8002);

        alloc.release(p2);
        alloc.release(p1);

        // Should get back 8000 first (lowest), then 8001
        assert_eq!(alloc.allocate().unwrap(), 8000);
        assert_eq!(alloc.allocate().unwrap(), 8001);
        // Then continues from where it left off
        assert_eq!(alloc.allocate().unwrap(), 8003);
    }

    #[test]
    fn reclaimed_allows_allocation_after_exhaustion() {
        let mut alloc = PortAllocator::new(8000, 8001);

        let p1 = alloc.allocate().unwrap();
        let p2 = alloc.allocate().unwrap();
        assert!(alloc.allocate().is_err());

        alloc.release(p1);
        assert_eq!(alloc.allocate().unwrap(), 8000);

        alloc.release(p2);
        assert_eq!(alloc.allocate().unwrap(), 8001);
    }
}
