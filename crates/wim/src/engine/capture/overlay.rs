//! In-memory capture overlays retain explicit stream ownership during remapping.
use super::{CapturePlan, CapturedSource};
use wim_format::{ParseError, metadata_write::OwnedDentry};

fn component(bytes: &[u8], units: &[u16], folded: bool) -> bool {
    let fold = |u| {
        if folded {
            wim_format::ntfs_upcase::uppercase(u)
        } else {
            u
        }
    };
    bytes
        .chunks_exact(2)
        .map(|b| fold(u16::from_le_bytes([b[0], b[1]])))
        .eq(units.iter().copied().map(fold))
}
fn find(plan: &CapturePlan, parent: usize, name: &[u16]) -> Option<usize> {
    plan.tree.nodes[parent]
        .children
        .iter()
        .copied()
        .find(|&i| component(&plan.tree.nodes[i].name, name, false))
        .or_else(|| {
            crate::engine::runtime::ignore_case()
                .then(|| {
                    plan.tree.nodes[parent]
                        .children
                        .iter()
                        .copied()
                        .find(|&i| component(&plan.tree.nodes[i].name, name, true))
                })
                .flatten()
        })
}
fn filler(name: Vec<u8>) -> OwnedDentry {
    let mut node = OwnedDentry::new(name, 0x10);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp =
        (now.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(now.subsec_nanos()) / 100;
    node.creation_time = timestamp;
    node.last_write_time = timestamp;
    node.last_access_time = timestamp;
    node
}
fn diagnostic(plan: &CapturePlan, index: usize) -> Result<Vec<u8>, ParseError> {
    let mut components = Vec::new();
    let mut node = index;
    while node != 0 {
        components.push(&plan.tree.nodes[node].name);
        node = plan
            .tree
            .nodes
            .iter()
            .position(|n| n.children.contains(&node))
            .ok_or(ParseError::InvalidMetadataResource)?;
    }
    let mut units = vec![47];
    for (i, bytes) in components.into_iter().rev().enumerate() {
        if i != 0 {
            units.push(47);
        }
        units.extend(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]])),
        );
    }
    wim_format::platform_text::utf16_to_wtf8(&units)
}
fn merge(
    plan: &mut CapturePlan,
    new: usize,
    old: usize,
    flags: i32,
    replace: &mut dyn FnMut(&[u8]) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let new_dir = plan.tree.nodes[new].attributes & 0x10 != 0;
    let old_dir = plan.tree.nodes[old].attributes & 0x10 != 0;
    if new_dir != old_dir {
        return Err(if old_dir {
            ParseError::IsDirectory
        } else {
            ParseError::Notdir
        });
    }
    if new_dir {
        let children = std::mem::take(&mut plan.tree.nodes[new].children);
        for child in children {
            let name: Vec<_> = plan.tree.nodes[child]
                .name
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            if let Some(existing) = find(plan, old, &name) {
                merge(plan, child, existing, flags, replace)?;
            } else {
                plan.tree.nodes[old].children.push(child);
            }
        }
    } else {
        if flags & 0x2000 != 0 {
            return Err(ParseError::InvalidOverlay);
        }
        if flags & 4 != 0 {
            replace(&diagnostic(plan, old)?)?;
        }
        let parent = plan
            .tree
            .nodes
            .iter()
            .position(|n| n.children.contains(&old))
            .ok_or(ParseError::InvalidMetadataResource)?;
        let position = plan.tree.nodes[parent]
            .children
            .iter()
            .position(|&i| i == old)
            .ok_or(ParseError::InvalidMetadataResource)?;
        plan.tree.nodes[parent].children[position] = new;
    }
    Ok(())
}
impl CapturePlan {
    /// Compact reachable nodes, retaining and remapping pending stream bindings.
    pub fn compact(&mut self) -> Result<(), ParseError> {
        let mut mapping = vec![usize::MAX; self.tree.nodes.len()];
        let mut order = Vec::new();
        let mut stack = if mapping.is_empty() {
            Vec::new()
        } else {
            vec![0]
        };
        while let Some(node) = stack.pop() {
            if node >= mapping.len() || mapping[node] != usize::MAX {
                return Err(ParseError::InvalidMetadataResource);
            }
            mapping[node] = order.len();
            order.push(node);
            stack.extend(self.tree.nodes[node].children.iter().rev().copied());
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(order.len())
            .map_err(|_| ParseError::Nomem)?;
        let mut identities = Vec::new();
        for index in order {
            let mut node = self.tree.nodes[index].clone();
            for child in &mut node.children {
                *child = mapping[*child];
            }
            nodes.push(node);
            identities.push(self.identities.get(index).copied().flatten());
        }
        self.tree.nodes = nodes;
        self.identities = identities;
        self.bindings.retain_mut(|b| {
            let next = mapping.get(b.node).copied().unwrap_or(usize::MAX);
            if next == usize::MAX {
                false
            } else {
                b.node = next;
                true
            }
        });
        Ok(())
    }
    /// Overlay one scanned branch at canonical image path components. Call on a
    /// cloned plan so errors/callback aborts cannot mutate a transaction checkpoint.
    pub fn overlay(
        &mut self,
        mut branch: Self,
        target: &[Vec<u16>],
        flags: i32,
        replace: &mut dyn FnMut(&[u8]) -> Result<(), ParseError>,
    ) -> Result<(), ParseError> {
        if branch.tree.nodes.is_empty() {
            return Ok(());
        }
        // Attaching a branch sets its target filename. Upstream's name setter
        // always removes the source DOS name, even for a nonempty target.
        branch.tree.nodes[0].short_name.clear();
        if target.is_empty() {
            branch.tree.nodes[0].name.clear();
        }
        if self.tree.nodes.is_empty() && target.is_empty() {
            *self = branch;
            return Ok(());
        }
        if self.tree.nodes.is_empty() {
            self.tree.nodes.push(filler(Vec::new()));
            self.identities.push(None);
        }
        // Incoming security IDs belong to the branch table, not the existing
        // image table. Deduplicate bytes and remap before moving any dentries.
        let mut security_ids = Vec::new();
        security_ids
            .try_reserve_exact(branch.tree.security_descriptors.len())
            .map_err(|_| ParseError::Nomem)?;
        for descriptor in branch.tree.security_descriptors.drain(..) {
            let index = if let Some(index) = self
                .tree
                .security_descriptors
                .iter()
                .position(|existing| *existing == descriptor)
            {
                index
            } else {
                self.tree
                    .security_descriptors
                    .try_reserve(1)
                    .map_err(|_| ParseError::Nomem)?;
                let index = self.tree.security_descriptors.len();
                self.tree.security_descriptors.push(descriptor);
                index
            };
            security_ids
                .push(u32::try_from(index).map_err(|_| ParseError::InvalidMetadataResource)?);
        }
        for node in &mut branch.tree.nodes {
            if node.security_id != u32::MAX {
                node.security_id = *security_ids
                    .get(node.security_id as usize)
                    .ok_or(ParseError::InvalidMetadataResource)?;
            }
        }
        self.identities.resize(self.tree.nodes.len(), None);
        let base = self.tree.nodes.len();
        for node in &mut branch.tree.nodes {
            for child in &mut node.children {
                *child += base;
            }
            if node.attributes & 0x400 == 0 && node.inode_union != 0 {
                node.inode_union = node
                    .inode_union
                    .checked_add(base as u64)
                    .ok_or(ParseError::InvalidMetadataResource)?;
            }
        }
        branch.tree.nodes[0].name = target
            .last()
            .map(|n| n.iter().flat_map(|u| u.to_le_bytes()).collect())
            .unwrap_or_default();
        for (index, identity) in branch.identities.iter().enumerate() {
            if branch.tree.nodes[index].attributes & (0x10 | 0x400) != 0 {
                continue;
            }
            if let Some(identity) = identity
                && let Some(first) = self
                    .identities
                    .iter()
                    .position(|old| old.as_ref() == Some(identity))
            {
                let group = if self.tree.nodes[first].inode_union != 0 {
                    self.tree.nodes[first].inode_union
                } else {
                    first as u64 + 1
                };
                self.tree.nodes[first].inode_union = group;
                branch.tree.nodes[index].inode_union = group;
            }
        }
        for binding in &mut branch.bindings {
            binding.node += base;
            if matches!(binding.stream.source, CapturedSource::File(_))
                && let Some(previous) = self
                    .bindings
                    .iter()
                    .find(|b| b.stream.identity == binding.stream.identity)
            {
                binding.stream = previous.stream.clone();
                let first = previous.node;
                let group = if self.tree.nodes[first].inode_union != 0 {
                    self.tree.nodes[first].inode_union
                } else {
                    first as u64 + 1
                };
                self.tree.nodes[first].inode_union = group;
                branch.tree.nodes[binding.node - base].inode_union = group;
            }
        }
        self.tree.nodes.extend(branch.tree.nodes);
        self.identities.extend(branch.identities);
        self.bindings.extend(branch.bindings);
        if target.is_empty() {
            merge(self, base, 0, flags, replace)?;
        } else {
            let mut parent = 0;
            for part in &target[..target.len() - 1] {
                if self.tree.nodes[parent].attributes & 0x10 == 0 {
                    return Err(ParseError::Notdir);
                }
                parent = if let Some(index) = find(self, parent, part) {
                    index
                } else {
                    let index = self.tree.nodes.len();
                    self.tree
                        .nodes
                        .push(filler(part.iter().flat_map(|u| u.to_le_bytes()).collect()));
                    self.identities.push(None);
                    self.tree.nodes[parent].children.push(index);
                    index
                };
            }
            if self.tree.nodes[parent].attributes & 0x10 == 0 {
                return Err(ParseError::Notdir);
            }
            if let Some(old) = find(self, parent, target.last().ok_or(ParseError::InvalidParam)?) {
                merge(self, base, old, flags, replace)?;
            } else {
                self.tree.nodes[parent].children.push(base);
            }
        }
        self.compact()
    }
}
