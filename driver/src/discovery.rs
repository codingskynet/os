//! Allocation-free FDT queries shared by device probes.
use arrayvec::ArrayVec;
use runtime::dev::dt::{Fdt, FdtToken, FdtWalker};

pub type Error = &'static str;

pub fn compatible(node: FdtWalker<'_>, name: &str) -> bool {
    node.prop("compatible").is_some_and(|value| {
        let bytes = value.into_slice();
        bytes.last() == Some(&0)
            && bytes
                .split(|b| *b == 0)
                .any(|entry| entry == name.as_bytes())
    })
}

/// Visit enabled nodes, including checking the status of their ancestors.
pub fn visit<'a>(
    fdt: &'a Fdt,
    mut visitor: impl FnMut(FdtWalker<'a>) -> Result<(), Error>,
) -> Result<(), Error> {
    fn enabled(node: FdtWalker<'_>) -> bool {
        node.prop("status")
            .is_none_or(|value| matches!(value.into_str(), Some("ok" | "okay")))
    }
    let mut walker = fdt.query();
    let mut parents = ArrayVec::<bool, 32>::new();
    parents.push(enabled(walker.clone()));
    if parents[0] {
        visitor(walker.clone())?;
    }
    while let Some(token) = walker.next() {
        match token {
            FdtToken::Node(_) => {
                let active = parents.last().copied().unwrap_or(false) && enabled(walker.clone());
                parents
                    .try_push(active)
                    .map_err(|_| "device tree nesting too deep")?;
                if active {
                    visitor(walker.clone())?;
                }
            }
            FdtToken::NodeEnd => {
                parents.pop();
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn unique<'a>(
    fdt: &'a Fdt,
    mut matches: impl FnMut(FdtWalker<'a>) -> bool,
) -> Result<Option<FdtWalker<'a>>, Error> {
    let mut found = None;
    visit(fdt, |node| {
        if matches(node.clone()) && found.replace(node).is_some() {
            return Err("multiple matching devices for a singleton service");
        }
        Ok(())
    })?;
    Ok(found)
}

pub fn phandle(fdt: &Fdt, handle: u32) -> Result<FdtWalker<'_>, Error> {
    unique(fdt, |node| {
        node.prop("phandle").and_then(|v| v.into_scalar_u32()) == Some(handle)
    })?
    .ok_or("interrupt parent is absent or disabled")
}

pub fn sstc_supported(fdt: &Fdt) -> Result<bool, Error> {
    let mut count = 0;
    let mut supported = true;
    visit(fdt, |node| {
        if node.clone().prop("device_type").and_then(|v| v.into_str()) != Some("cpu") {
            return Ok(());
        }
        count += 1;
        let sstc = if let Some(value) = node.clone().prop("riscv,isa-extensions") {
            let bytes = value.into_slice();
            bytes.last() == Some(&0) && bytes.split(|b| *b == 0).any(|s| s == b"sstc")
        } else {
            node.prop("riscv,isa")
                .and_then(|v| v.into_str())
                .is_some_and(|isa| {
                    isa.split('_').any(|extension| {
                        extension == "sstc"
                            || extension.strip_prefix("sstc").is_some_and(|version| {
                                version.starts_with(|c: char| c.is_ascii_digit())
                            })
                    })
                })
        };
        supported &= sstc;
        Ok(())
    })?;
    Ok(count != 0 && supported)
}

pub fn path_enabled(fdt: &Fdt, mut path: &str) -> bool {
    loop {
        if fdt
            .lookup(path)
            .prop("status")
            .is_some_and(|v| !matches!(v.into_str(), Some("ok" | "okay")))
        {
            return false;
        }
        let Some((parent, _)) = path.rsplit_once('/') else {
            return true;
        };
        path = parent;
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    /// Small DTB writer for probe topology tests (no dtc dependency).
    struct Tree {
        structure: Vec<u8>,
        strings: Vec<u8>,
    }
    impl Tree {
        fn new() -> Self {
            let mut tree = Self {
                structure: Vec::new(),
                strings: Vec::new(),
            };
            tree.node("");
            tree
        }
        fn word(&mut self, word: u32) {
            self.structure.extend(word.to_be_bytes());
        }
        fn pad(&mut self) {
            while self.structure.len() % 4 != 0 {
                self.structure.push(0);
            }
        }
        fn node(&mut self, name: &str) {
            self.word(1);
            self.structure.extend(name.as_bytes());
            self.structure.push(0);
            self.pad();
        }
        fn end(&mut self) {
            self.word(2);
        }
        fn prop(&mut self, name: &str, value: &[u8]) {
            let offset = self.strings.len();
            self.strings.extend(name.as_bytes());
            self.strings.push(0);
            self.word(3);
            self.word(value.len() as u32);
            self.word(offset as u32);
            self.structure.extend(value);
            self.pad();
        }
        fn cpu(&mut self, name: &str, isa: &[u8]) {
            self.node(name);
            self.prop("device_type", b"cpu\0");
            self.prop("riscv,isa", isa);
            self.end();
        }
        fn finish(mut self) -> Vec<u8> {
            self.end();
            self.word(9);
            let strings_offset = 56 + self.structure.len();
            let total = strings_offset + self.strings.len();
            let mut bytes = Vec::new();
            for value in [
                0xd00dfeed,
                total as u32,
                56,
                strings_offset as u32,
                40,
                17,
                16,
                0,
                self.strings.len() as u32,
                self.structure.len() as u32,
            ] {
                bytes.extend(value.to_be_bytes());
            }
            bytes.extend([0; 16]);
            bytes.extend(self.structure);
            bytes.extend(self.strings);
            bytes
        }
    }

    #[test]
    fn qemu_devices_are_found_without_board_identity() {
        let fdt = runtime::dev::dt::tests::qemu_fdt();
        assert!(sstc_supported(&fdt).unwrap());
        let console = runtime::dev::console::ConsoleConfig::from_fdt(&fdt).unwrap();
        let node = phandle(&fdt, console.interrupt().unwrap().0).unwrap();
        assert!(compatible(node, "sifive,plic-1.0.0"));
        assert!(path_enabled(&fdt, console.path()));
    }

    #[test]
    fn disabled_devices_and_disabled_buses_are_not_probed() {
        let mut tree = Tree::new();
        tree.node("bus");
        tree.prop("status", b"disabled\0");
        tree.node("timer");
        tree.prop("compatible", b"litex,timer\0");
        tree.end();
        tree.end();
        tree.node("off");
        tree.prop("compatible", b"litex,timer\0");
        tree.prop("status", b"disabled\0");
        tree.end();
        tree.node("on");
        tree.prop("compatible", b"litex,timer\0");
        tree.prop("phandle", &7u32.to_be_bytes());
        tree.end();
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        let node = unique(&fdt, |n| compatible(n, "litex,timer"))
            .unwrap()
            .unwrap();
        assert_eq!(node.prop("phandle").unwrap().into_scalar_u32(), Some(7));
        assert!(!path_enabled(&fdt, "/bus/timer"));
    }

    #[test]
    fn multiple_matching_timers_are_rejected() {
        let mut tree = Tree::new();
        for name in ["timer0", "timer1"] {
            tree.node(name);
            tree.prop("compatible", b"litex,timer\0");
            tree.end();
        }
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        assert!(unique(&fdt, |n| compatible(n, "litex,timer")).is_err());
    }

    #[test]
    fn compatible_aliases_do_not_duplicate_one_device() {
        let mut tree = Tree::new();
        tree.node("intc");
        tree.prop("compatible", b"vendor,specific\0riscv,plic0\0");
        tree.end();
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        assert!(
            unique(&fdt, |n| compatible(n.clone(), "vendor,specific")
                || compatible(n, "riscv,plic0"))
            .unwrap()
            .is_some()
        );
        assert!(
            unique(&fdt, |n| compatible(n, "vexriscv,intc0"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn duplicate_and_disabled_interrupt_parents_are_rejected() {
        let mut tree = Tree::new();
        for name in ["intc0", "intc1"] {
            tree.node(name);
            tree.prop("phandle", &1u32.to_be_bytes());
            tree.end();
        }
        tree.node("off");
        tree.prop("phandle", &2u32.to_be_bytes());
        tree.prop("status", b"disabled\0");
        tree.end();
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        assert!(phandle(&fdt, 1).is_err());
        assert!(phandle(&fdt, 2).is_err());
    }

    #[test]
    fn sstc_requires_all_enabled_cpus_to_support_it() {
        let mut tree = Tree::new();
        tree.node("cpus");
        tree.cpu("cpu@0", b"rv64ima_sstc1p0\0");
        tree.cpu("cpu@1", b"rv64ima\0");
        tree.end();
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        assert!(!sstc_supported(&fdt).unwrap());
    }

    #[test]
    fn sstc_uses_extension_list_and_ignores_disabled_cpus() {
        let mut tree = Tree::new();
        tree.node("cpus");
        tree.node("cpu@0");
        tree.prop("device_type", b"cpu\0");
        tree.prop("riscv,isa-extensions", b"i\0m\0sstc\0");
        tree.end();
        tree.node("cpu@1");
        tree.prop("device_type", b"cpu\0");
        tree.prop("status", b"disabled\0");
        tree.end();
        tree.end();
        let bytes = tree.finish();
        let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
        assert!(sstc_supported(&fdt).unwrap());
    }
    #[test]
    fn legacy_isa_matches_extension_tokens_not_prefixes() {
        for (isa, expected) in [
            (&b"rv64ima_sstc\0"[..], true),
            (&b"rv64ima_sstc1p0\0"[..], true),
            (&b"rv64ima_sstcx\0"[..], false),
        ] {
            let mut tree = Tree::new();
            tree.node("cpus");
            tree.cpu("cpu@0", isa);
            tree.end();
            let bytes = tree.finish();
            let fdt = unsafe { Fdt::new(bytes.as_ptr()).unwrap() };
            assert_eq!(sstc_supported(&fdt).unwrap(), expected);
        }
    }
}
