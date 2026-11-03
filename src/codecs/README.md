# Model-specific serde JSON codecs

`RustJsonEmitter` implements `JsonCodecEmitter` and emits `json_codecs.rs` with
`read_<RustModelName>_json` and `write_<RustModelName>_json` functions. The consumer
module root exposes sibling `models` (08c), `contracts` (08a) and `values` (08d)
modules. A downstream crate can re-export contracts/values from lutaml-model once
08f wires those public modules. Generated consumers need serde (derive), pinned
serde_json with arbitrary_precision, and the exact-value dependencies.

Readers lower declared aliases and nested model/discriminator mappings into the
static generated structs, preserve absent/null/value, and run model validation.
They do not automatically apply defaults, which would erase absence. Writers
apply declared nil/empty/default render policies and use the first declared alias.
Missing render policies resolve to Ruby's false/omit policy for all three options.
Exact values remain strings; merged 08d helpers validate integer/decimal lexical
boundaries without floating conversion. Model fields remain typed; JSON values
are only temporary input/output buffers. Schema declarations are compiled out.

Mapping-level root/child descriptors bind the single unambiguous mapped model
collection. `key` names its object key, `value` names the whole object value, and
string/array paths lower nested values. A key-only descriptor preserves the child
model's remaining JSON mappings; additional explicit paths replace them. Ambiguous
collection targets, missing keys, overlapping paths, custom codec bindings without
a callable signature, and keyed discriminator collections reject explicitly.
Root keyed descriptors with ordered-content metadata reject until an envelope is
declared. Ordinary ordered-content token lists round-trip with field mappings.

The focused tests compile and run an actual generated consumer in target/. The
outer Cargo invocation must hold the host memory lock; the child build uses one
worker and shares that lease.

04a corpus replay is a delivery dependency. Preparation tests do not claim oracle
coverage; pin and execute the actual merged case IDs before marking 08e complete.
The explicit portable profile requires exact-value strings. Native Ruby cases
retain their declared native profile; profile selection must account for every
required case ID instead of silently skipping incompatible wires.

Model names come directly from `generator::model_names`; normalization is shared
with actual structs, enums and PARG constructors.
