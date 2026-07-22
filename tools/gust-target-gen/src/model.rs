//! Target model: parses `spar items --format json` output (AADL hardware
//! model) into a flat `Target` struct that later tasks feed into emitters
//! (Rust consts, memory.x, WIT worlds).

use std::collections::BTreeMap;

use serde::Deserialize;

/// One `Gust_Target_Props::X` property attached to a component type/impl,
/// as `spar items --format json` emits it. `typed_value` is left as raw
/// JSON (`{"Integer": [v, hex-or-null]}` / `{"Boolean": b}` / other kinds
/// this parser doesn't care about yet) rather than a derived enum, so
/// property kinds this parser doesn't handle don't break deserialization
/// of unrelated components.
#[derive(Debug, Deserialize)]
struct RawProperty {
    name: String,
    typed_value: serde_json::Value,
}

impl RawProperty {
    fn as_integer(&self) -> Option<u32> {
        self.typed_value
            .get("Integer")
            .and_then(|v| v.get(0))
            .and_then(|v| v.as_i64())
            .map(|v| v as u32)
    }

    fn as_boolean(&self) -> Option<bool> {
        self.typed_value.get("Boolean").and_then(|v| v.as_bool())
    }
}

#[derive(Debug, Deserialize)]
struct RawComponentType {
    name: String,
    #[serde(default)]
    properties: Vec<RawProperty>,
}

#[derive(Debug, Deserialize)]
struct RawSubcomponent {
    name: String,
    category: String,
    classifier: String,
}

#[derive(Debug, Deserialize)]
struct RawComponentImpl {
    name: String,
    #[serde(default)]
    subcomponents: Vec<RawSubcomponent>,
    #[serde(default)]
    properties: Vec<RawProperty>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    component_types: Vec<RawComponentType>,
    component_impls: Vec<RawComponentImpl>,
}

/// Strip the `Gust_Target_Props::` (or any `Foo::`) qualifier from a
/// property name, leaving just `Csr_Offset`.
fn unqualified(prop_name: &str) -> &str {
    prop_name.rsplit("::").next().unwrap_or(prop_name)
}

/// Read every integer property (unqualified name -> value) plus the `Base`
/// and `Present` properties out of a raw property list.
fn read_props(properties: &[RawProperty]) -> (Option<u32>, bool, BTreeMap<String, u32>) {
    let mut base = None;
    let mut present = false;
    let mut props = BTreeMap::new();
    for p in properties {
        let key = unqualified(&p.name);
        if let Some(v) = p.as_integer() {
            if key == "Base" {
                base = Some(v);
            } else {
                props.insert(key.to_string(), v);
            }
        } else if let Some(b) = p.as_boolean() {
            if key == "Present" {
                present = b;
            }
        }
    }
    (base, present, props)
}

/// Resolve a subcomponent `classifier` (e.g. `"Cortex_M::CortexM3"`,
/// `"Flag_Flash.i"`, `"Iwdg"`) to the properties of the component type or
/// implementation it names, searching `home_package` first and falling
/// back to the package named by an explicit `Pkg::` qualifier.
fn resolve_classifier<'a>(
    packages: &'a [RawPackage],
    home_package: &'a RawPackage,
    classifier: &str,
) -> &'a [RawProperty] {
    let (pkg, local) = match classifier.split_once("::") {
        Some((pkg_name, local)) => (
            packages
                .iter()
                .find(|p| p.name == pkg_name)
                .unwrap_or_else(|| panic!("gust-target-gen: unknown package `{pkg_name}` referenced by classifier `{classifier}`")),
            local,
        ),
        None => (home_package, classifier),
    };

    // An impl name always contains a `.` (Type.impl); a bare type name never
    // does (AADL identifiers can't contain '.').
    if local.contains('.') {
        &pkg.component_impls
            .iter()
            .find(|i| i.name == local)
            .unwrap_or_else(|| panic!("gust-target-gen: unknown component impl `{local}` in package `{}` (classifier `{classifier}`)", pkg.name))
            .properties
    } else {
        &pkg.component_types
            .iter()
            .find(|t| t.name == local)
            .unwrap_or_else(|| panic!("gust-target-gen: unknown component type `{local}` in package `{}` (classifier `{classifier}`)", pkg.name))
            .properties
    }
}

/// A base+length memory region (flash, sram).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub base: u32,
    pub len: u32,
}

/// A memory-mapped device (peripheral) on the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub base: u32,
    pub present: bool,
    /// Remaining integer properties, keyed by their unqualified name
    /// (`Gust_Target_Props::Csr_Offset` -> `Csr_Offset`). Does not include
    /// `Base`/`Present`, which have dedicated fields above.
    pub props: BTreeMap<String, u32>,
}

/// A fully-resolved hardware target: one board `system implementation`
/// with its memory regions and devices resolved from the AADL model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub cpu: String,
    pub flash: Region,
    pub sram: Region,
    pub devices: Vec<Device>,
}

/// Parse `spar items --format json` output and resolve the named board
/// `system implementation` (e.g. `"STM32F100::Board.vldiscovery"`, given as
/// `"<package>::<Type.impl>"`) into a `Target`: its `cpu`, `flash`/`sram`
/// regions, and `device`-category subcomponents with their properties.
///
/// Panics on any shape spar didn't actually produce (unknown package/board,
/// unresolvable classifier, malformed JSON) — this is a build-time codegen
/// tool, not a service, so a loud early failure beats a silently wrong
/// `Target`.
pub fn parse_items(json: &str, board_impl: &str) -> Target {
    let packages: Vec<RawPackage> =
        serde_json::from_str(json).expect("gust-target-gen: malformed spar items JSON");

    let (package_name, impl_local) = board_impl.split_once("::").unwrap_or_else(|| {
        panic!("gust-target-gen: board_impl `{board_impl}` must be `<package>::<Type.impl>`")
    });

    let package = packages
        .iter()
        .find(|p| p.name == package_name)
        .unwrap_or_else(|| panic!("gust-target-gen: unknown package `{package_name}`"));

    let board = package
        .component_impls
        .iter()
        .find(|i| i.name == impl_local)
        .unwrap_or_else(|| panic!("gust-target-gen: unknown component impl `{impl_local}` in package `{package_name}`"));

    let mut cpu = None;
    let mut flash = None;
    let mut sram = None;
    let mut devices = Vec::new();

    for sub in &board.subcomponents {
        let properties = resolve_classifier(&packages, package, &sub.classifier);
        let (base, present, props) = read_props(properties);

        match sub.category.as_str() {
            "processor" => cpu = Some(sub.classifier.clone()),
            "memory" => {
                let region = Region {
                    base: base.unwrap_or_else(|| {
                        panic!(
                            "gust-target-gen: memory subcomponent `{}` has no Base property",
                            sub.name
                        )
                    }),
                    len: *props.get("Length").unwrap_or_else(|| {
                        panic!(
                            "gust-target-gen: memory subcomponent `{}` has no Length property",
                            sub.name
                        )
                    }),
                };
                match sub.name.as_str() {
                    "flash" => flash = Some(region),
                    "sram" => sram = Some(region),
                    other => panic!("gust-target-gen: unexpected memory subcomponent `{other}` (expected flash/sram)"),
                }
            }
            "device" => devices.push(Device {
                name: sub.classifier.clone(),
                base: base.unwrap_or_else(|| {
                    panic!(
                        "gust-target-gen: device subcomponent `{}` has no Base property",
                        sub.name
                    )
                }),
                present,
                props,
            }),
            _ => {}
        }
    }

    Target {
        name: board_impl.to_string(),
        cpu: cpu.unwrap_or_else(|| {
            panic!("gust-target-gen: board `{board_impl}` has no processor subcomponent")
        }),
        flash: flash.unwrap_or_else(|| {
            panic!("gust-target-gen: board `{board_impl}` has no flash subcomponent")
        }),
        sram: sram.unwrap_or_else(|| {
            panic!("gust-target-gen: board `{board_impl}` has no sram subcomponent")
        }),
        devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iwdg_base_and_rcc_offset_from_spar_json() {
        // minimal spar `items --format json` shape: packages[].component_types[]
        let json = include_str!("../tests/golden/f100.items.json");
        let t = parse_items(json, "STM32F100::Board.vldiscovery");
        assert_eq!(t.flash.base, 0x0800_0000);
        assert_eq!(t.flash.len, 131072);
        let rcc = t.devices.iter().find(|d| d.name == "Rcc").unwrap();
        assert_eq!(rcc.props["Csr_Offset"], 0x24);
        assert_eq!(rcc.props["Rmvf_Bit"], 24);
        let iwdg = t.devices.iter().find(|d| d.name == "Iwdg").unwrap();
        assert_eq!(iwdg.base, 0x4000_3000);
        assert!(iwdg.present);
    }
}
