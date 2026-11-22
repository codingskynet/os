//! Discover driver sources for the full library and selective board builds.
#![allow(clippy::large_stack_frames)]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Group, Ident, TokenStream, TokenTree};
use quote::quote;
use syn::{Attribute, File, Item};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
pub enum Output {
    Library,
    Selection,
}

fn read_source(path: &Path) -> Result<File> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    syn::parse_file(&text).map_err(|error| format!("{}: {error}", path.display()).into())
}

fn entries(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

// A category contains driver entry files. The driver owns its helper directory.
fn driver_files(category: &Path) -> Result<Vec<PathBuf>> {
    Ok(entries(category)?
        .into_iter()
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| ext == "rs")
                && !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("lib.rs" | "main.rs" | "mod.rs")
                )
        })
        .collect())
}

fn module_name(path: &Path) -> Result<Ident> {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or("non-UTF8 module name")?;
    // Match the module identifiers produced by automod::dir!.
    let mut name = stem.replace('-', "_");
    if name.starts_with(|ch: char| ch.is_ascii_digit()) {
        name.insert(0, '_');
    }
    Ok(syn::parse_str(&name)?)
}

fn guards(attrs: &[Attribute]) -> impl Iterator<Item = Attribute> + '_ {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
        .cloned()
        .map(|mut attr| {
            attr.style = syn::AttrStyle::Outer;
            attr
        })
}

// `$` inside a driver's own macro must survive the generated outer macro.
// Each expansion captures a literal dollar token as `$d` before emitting code.
fn escape_dollars(tokens: TokenStream) -> TokenStream {
    tokens
        .into_iter()
        .flat_map(|token| match token {
            TokenTree::Punct(ref punct) if punct.as_char() == '$' => quote! { $d },
            TokenTree::Group(group) => {
                let mut escaped = Group::new(group.delimiter(), escape_dollars(group.stream()));
                escaped.set_span(group.span());
                TokenStream::from(TokenTree::Group(escaped))
            }
            token => TokenStream::from(token),
        })
        .collect()
}

#[derive(Default)]
struct Catalog {
    consoles: Vec<TokenStream>,
    timers: Vec<TokenStream>,
    interrupts: Vec<TokenStream>,
}

impl Catalog {
    fn register(&mut self, category: &Ident, name: &Ident, source: &File) -> Result<()> {
        for item in &source.items {
            let Item::Const(item) = item else { continue };
            if item.ident != "DRIVER" {
                continue;
            }
            let syn::Type::Path(ty) = &*item.ty else {
                return Err(
                    format!("{category}/{name}: DRIVER must have a descriptor type").into(),
                );
            };
            let entries = match ty.path.segments.last().unwrap().ident.to_string().as_str() {
                "ConsoleDriver" => &mut self.consoles,
                "TimerDriver" => &mut self.timers,
                "InterruptDriver" => &mut self.interrupts,
                other => {
                    return Err(
                        format!("{category}/{name}: unknown DRIVER descriptor `{other}`").into(),
                    );
                }
            };
            let conditions = guards(&source.attrs).chain(guards(&item.attrs));
            entries.push(quote! { #(#conditions)* crate::#category::#name::DRIVER });
        }
        Ok(())
    }
}

// Preserve Rust's module visibility, attributes and item order in a board-local
// expansion. Resolve external declarations from the driver, even when aliased.
fn inline_modules(
    items: &mut [Item],
    directory: &Path,
    source_dir: &Path,
    ancestors: &mut Vec<PathBuf>,
) -> Result<()> {
    for item in items {
        let Item::Mod(module) = item else { continue };
        let child = directory.join(module.ident.to_string().trim_start_matches("r#"));
        if let Some((_, items)) = &mut module.content {
            inline_modules(items, &child, &child, ancestors)?;
            continue;
        }
        let explicit_path = module
            .attrs
            .iter()
            .position(|attr| attr.path().is_ident("path"));
        let path = if let Some(index) = explicit_path {
            let attr = module.attrs.remove(index);
            let syn::Meta::NameValue(value) = attr.meta else {
                return Err("invalid module path".into());
            };
            let literal: syn::LitStr =
                syn::parse2(quote::ToTokens::into_token_stream(value.value))?;
            source_dir.join(literal.value())
        } else {
            let file = child.with_extension("rs");
            let root = child.join("mod.rs");
            match (file.is_file(), root.is_file()) {
                (true, true) => {
                    return Err(format!(
                        "ambiguous module: {} and {}",
                        file.display(),
                        root.display()
                    )
                    .into());
                }
                (_, true) => root,
                _ => file,
            }
        };
        let nested = if explicit_path.is_some() {
            path.parent().unwrap()
        } else {
            &child
        };
        // Defer file errors until this module is selected and its cfg is active.
        let file = inline_file(&path, nested, ancestors).unwrap_or_else(|error| {
            let message = error.to_string();
            syn::parse_quote! { compile_error!(#message); }
        });
        module.attrs.extend(file.attrs);
        module.content = Some((Default::default(), file.items));
        module.semi = None;
    }
    Ok(())
}

fn inline_file(path: &Path, directory: &Path, ancestors: &mut Vec<PathBuf>) -> Result<File> {
    let mut file = read_source(path)?;
    let canonical = fs::canonicalize(path)?;
    if ancestors.contains(&canonical) {
        return Err(format!("circular module: {}", path.display()).into());
    }
    ancestors.push(canonical);
    let result = inline_modules(
        &mut file.items,
        directory,
        path.parent().unwrap(),
        ancestors,
    );
    ancestors.pop();
    result?;
    Ok(file)
}

fn selection_arm(key: &str, name: &Ident, body: TokenStream) -> TokenStream {
    let body = escape_dollars(body);
    quote! {
        (#key) => { use_driver!(@source #key, #name, $); };
        (#key as $name:ident) => { use_driver!(@source #key, $name, $); };
        (@source #key, $name:ident, $d:tt) => { pub mod $name { #body } };
    }
}

fn generate(directory: &Path, destination: &Path, output: Output) -> Result<()> {
    let mut catalog = Catalog::default();
    let mut arms = TokenStream::new();
    for group in entries(directory)?.into_iter().filter(|path| path.is_dir()) {
        let category = module_name(&group)?;
        let mut members = TokenStream::new();
        for path in driver_files(&group)? {
            let name = module_name(&path)?;
            match output {
                Output::Library => catalog.register(&category, &name, &read_source(&path)?)?,
                Output::Selection => {
                    let key = format!("{category}/{name}");
                    let file = inline_file(&path, &path.with_extension(""), &mut Vec::new())?;
                    arms.extend(selection_arm(&key, &name, quote! { #file }));
                    members.extend(quote! { use_driver!(#key); });
                }
            }
        }
        if matches!(output, Output::Selection) {
            arms.extend(selection_arm(&category.to_string(), &category, members));
        }
    }

    let generated = match output {
        Output::Library => {
            let Catalog {
                consoles,
                timers,
                interrupts,
            } = catalog;
            quote! {
                pub struct AllDevices;
                impl boot::BootDevice for AllDevices {
                    const DRIVERS: boot::DriverSet = boot::DriverSet {
                        consoles: &[#(#consoles),*],
                        timers: &[#(#timers),*],
                        interrupts: &[#(#interrupts),*],
                    };
                }
            }
        }
        Output::Selection => {
            quote! {
                macro_rules! use_driver {
                    #arms
                    ($($invalid:tt)*) => { compile_error!(concat!("unknown driver selection: ", stringify!($($invalid)*))); };
                }
            }
        }
    };
    syn::parse2::<File>(generated.clone())?;
    fs::write(destination, generated.to_string())?;
    Ok(())
}

#[cfg(not(test))]
pub fn generate_for_cargo(directory: &str, output: Output) {
    let destination =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("drivers.rs");
    println!("cargo:rerun-if-changed={directory}");
    generate(Path::new(directory), &destination, output).expect("generate driver modules");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "os-driver-gen-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            Self(root)
        }
        fn write(&self, path: &str, source: &str) {
            let path = self.0.join("src").join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        fn compile(&self, mode: Output, body: &str) -> std::process::Output {
            let generated = self.0.join("drivers.rs");
            generate(&self.0.join("src"), &generated, mode).unwrap();
            let program = format!(
                r#"
                extern crate self as boot;
                pub struct ConsoleDriver(pub u32);
                pub struct TimerDriver;
                pub struct InterruptDriver;
                pub struct DriverSet {{
                    pub consoles: &'static [ConsoleDriver],
                    pub timers: &'static [TimerDriver],
                    pub interrupts: &'static [InterruptDriver],
                }}
                pub trait BootDevice {{ const DRIVERS: DriverSet; }}
                include!({generated:?});
                {body}
            "#
            );
            let path = self.0.join("src/lib.rs");
            fs::write(&path, program).unwrap();
            Command::new("rustc")
                .args(["--edition=2024", "--crate-name=fixture"])
                .arg(path)
                .arg("-o")
                .arg(self.0.join("fixture"))
                .output()
                .unwrap()
        }
        fn run(&self, mode: Output, body: &str) {
            let output = self.compile(mode, body);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                Command::new(self.0.join("fixture"))
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn selection_inlines_children_and_macros_without_compiling_other_drivers() {
        let fixture = Fixture::new();
        fixture.write(
            "uart/good.rs",
            r#"
            //! A driver with a private macro and a declared helper.
            mod registers;
            macro_rules! value { ($input:expr) => { $input }; }
            pub fn value() -> u32 { value!(registers::VALUE) }
        "#,
        );
        fixture.write("uart/good/registers.rs", "pub const VALUE: u32 = 7;");
        fixture.write("uart/broken.rs", "pub fn broken() { not_a_function(); }");
        fixture.run(
            Output::Selection,
            r#"use_driver!("uart/good" as uart); fn main() { assert_eq!(uart::value(), 7); }"#,
        );
        let output = fixture.compile(
            Output::Selection,
            r#"use_driver!("uart/broken"); fn main() {}"#,
        );
        assert!(
            !output.status.success(),
            "explicitly selecting the broken driver must fail"
        );
    }

    #[test]
    fn explicit_helpers_match_the_library_under_a_board_alias() {
        let fixture = Fixture::new();
        fixture.write(
            "interrupt/controller.rs",
            "mod registers; #[cfg(any())] mod absent; pub fn value() -> u32 { registers::value() }",
        );
        fixture.write(
            "interrupt/controller/registers.rs",
            "mod backend; pub(super) fn value() -> u32 { backend::VALUE }",
        );
        fixture.write("interrupt/controller/registers/backend/mod.rs", "macro_rules! sum { ($($val:expr),*) => { 0 $(+ $val)* }; } pub const VALUE: u32 = sum!(1, 2);");
        fixture.write(
            "interrupt/controller/unused.rs",
            "compile_error!(\"undeclared helper\");",
        );
        let library =
            "mod interrupt { pub mod controller; } use interrupt::controller as selected;";
        let selection = r#"use_driver!("interrupt/controller" as selected);"#;
        for (mode, import) in [(Output::Library, library), (Output::Selection, selection)] {
            fixture.run(
                mode,
                &format!("{import} fn main() {{ assert_eq!(selected::value(), 3); }}"),
            );
        }
    }

    #[test]
    fn all_drivers_discovers_additions_and_preserves_parent_and_descriptor_cfg() {
        let fixture = Fixture::new();
        fixture.write(
            "uart/first.rs",
            "pub const DRIVER: crate::ConsoleDriver = crate::ConsoleDriver(1);",
        );
        fixture.run(
            Output::Library,
            "mod uart { pub mod first; } fn main() { assert_eq!(AllDevices::DRIVERS.consoles.len(), 1); }",
        );
        fixture.write(
            "uart/second.rs",
            "pub const DRIVER: crate::ConsoleDriver = crate::ConsoleDriver(2);",
        );
        fixture.write("uart/helper.rs", "pub const VALUE: u32 = 9;");
        fixture.write(
            "uart/disabled.rs",
            "#![cfg(any())] pub const DRIVER: crate::ConsoleDriver = invalid();",
        );
        fixture.write(
            "uart/conditional.rs",
            "#[cfg(any())] pub const DRIVER: crate::ConsoleDriver = invalid();",
        );
        fixture.run(Output::Library, "mod uart { pub mod first; pub mod second; pub mod helper; pub mod disabled; pub mod conditional; } fn main() { assert_eq!(AllDevices::DRIVERS.consoles.len(), 2); assert_eq!(AllDevices::DRIVERS.consoles[1].0, 2); assert_eq!(uart::helper::VALUE, 9); }");
    }

    #[test]
    fn rejects_unknown_selection_and_ambiguous_module_paths() {
        let fixture = Fixture::new();
        fixture.write("uart/a.rs", "");
        let output = fixture.compile(
            Output::Selection,
            r#"use_driver!("uart/missing"); fn main() {}"#,
        );
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown driver selection"));
        fixture.write("uart/a.rs", "mod child;");
        fixture.write("uart/a/child.rs", "");
        fixture.write("uart/a/child/mod.rs", "");
        let error = generate(
            &fixture.0.join("src"),
            &fixture.0.join("drivers.rs"),
            Output::Selection,
        )
        .unwrap_err();
        assert!(error.to_string().contains("ambiguous module"));
    }
}
