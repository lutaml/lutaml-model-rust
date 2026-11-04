// Verify every byte before instantiating wasm or importing generated glue.
// Same pins and failure vocabulary as src/wasm/loader.mjs, for the XML artifact.
import { ArtifactError, digest } from '../../src/wasm/loader.mjs';
export { ArtifactError, digest };
const fail = (reason, location, detail) => { throw new ArtifactError(reason, location, detail); };
export const REQUIRED = ['mml_wasm.js', 'mml_wasm_bg.wasm', 'cases.json', 'oracle.json', 'mml-v3-mi-mixed.json', 'mml-v4-mi-mixed.json', 'mml-oracle.json',
  'oracle_models.rs', 'oracle_xml_codecs.rs', 'mml_v3_models.rs', 'mml_v3_xml_codecs.rs', 'mml_v4_models.rs', 'mml_v4_xml_codecs.rs'];
export async function loadArtifact({ manifestBytes, expectedDigest, read, importModule }) {
  if (await digest(manifestBytes) !== expectedDigest)
    fail('checksum_mismatch', '/manifest', 'manifest digest mismatch');
  let manifest;
  try { manifest = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(manifestBytes)); }
  catch { fail('invalid_document', '/manifest', 'invalid manifest JSON'); }
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest))
    fail('invalid_document', '/manifest', 'manifest object required');
  if (manifest.manifest !== 1 || manifest.schema_version !== 2 || manifest.generator_version !== 'lml-rust-wasm-xml-v1' || manifest.target !== 'wasm')
    fail('unsupported_version', '/manifest', 'unsupported wasm artifact version');
  if (!Array.isArray(manifest.capabilities) || !manifest.capabilities.includes('xml'))
    fail('capability_missing', '/capabilities', 'XML codec required');
  if (!Array.isArray(manifest.dependencies) || !manifest.dependencies.some(d => d && d.name === 'wasm-bindgen' && d.revision === '0.2.100' && /^sha256:[a-f0-9]{64}$/.test(d.sha256)))
    fail('unsupported_version', '/dependencies', 'wasm-bindgen 0.2.100 required');
  if (!Array.isArray(manifest.files) || !Array.isArray(manifest.corpora))
    fail('invalid_document', '/files', 'files and corpus pins required');
  const files = new Map();
  for (const entry of manifest.files) {
    if (!entry || typeof entry !== 'object' || !/^sha256:[a-f0-9]{64}$/.test(entry.sha256))
      fail('invalid_document', '/files', 'file digest required');
    const { path, sha256 } = entry;
    if (typeof path !== 'string' || !path || /[\\:%?#]/.test(path) || path.startsWith('/') || path.split('/').some(p => !p || p === '.' || p === '..') || files.has(path))
      fail('invalid_document', '/files', 'unsafe or duplicate artifact path');
    let bytes;
    try { bytes = await read(path); } catch { fail('checksum_mismatch', `/files/${path}`, 'missing artifact'); }
    if (await digest(bytes) !== sha256) fail('checksum_mismatch', `/files/${path}`, 'artifact digest mismatch');
    files.set(path, bytes);
  }
  for (const path of REQUIRED)
    if (!files.has(path)) fail('invalid_document', '/files', `required artifact ${path} missing`);
  if (!manifest.corpora.some(c => c && c.generation === 'wasm-xml-v1' && c.sha256 === manifest.files.find(f => f.path === 'cases.json').sha256))
    fail('checksum_mismatch', '/corpora', 'corpus pin mismatch');
  const text = path => new TextDecoder('utf-8', { fatal: true }).decode(files.get(path));
  const wasm = await importModule(text('mml_wasm.js'));
  await wasm.default({ module_or_path: files.get('mml_wasm_bg.wasm') });
  wasm.initialize(manifest.generator_version, manifest.lml_checksum);
  return { wasm, manifest, corpus: JSON.parse(text('cases.json')),
    bundles: { oracle: JSON.parse(text('oracle.json')), mml_v3: JSON.parse(text('mml-v3-mi-mixed.json')), mml_v4: JSON.parse(text('mml-v4-mi-mixed.json')) } };
}
