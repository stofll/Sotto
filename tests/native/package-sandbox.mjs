import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const marker = 'SOTTO_NATIVE_E2E_ONLY_V1';
const libraries = ['onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll'];
const crtLibraries = ['msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll'];
export const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
const xml = (value) => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');

export function sandboxXml(input, reports) {
  return `<Configuration>
  <Networking>Disable</Networking>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <PrinterRedirection>Disable</PrinterRedirection>
  <vGPU>Disable</vGPU>
  <MemoryInMB>8192</MemoryInMB>
  <MappedFolders>
    <MappedFolder><HostFolder>${xml(input)}</HostFolder><SandboxFolder>C:\\SottoInput</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>${xml(reports)}</HostFolder><SandboxFolder>C:\\SottoReports</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\\SottoInput\\bootstrap.ps1</Command></LogonCommand>
</Configuration>
`;
}

function filesUnder(directory, prefix = '') {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const relative = prefix + entry.name;
    if (entry.isSymbolicLink()) throw new Error(`Linked files are not allowed in the payload: ${relative}`);
    if (entry.isDirectory()) return filesUnder(join(directory, entry.name), relative + '/');
    if (!entry.isFile()) throw new Error(`Unsupported payload entry: ${relative}`);
    return [relative];
  });
}

export function createSandbox({ binaryDir, webviewDir, crtDir, output, nodeExe = process.execPath }) {
  binaryDir = resolve(binaryDir);
  webviewDir = resolve(webviewDir);
  crtDir = resolve(crtDir);
  output = resolve(output);
  if (existsSync(output)) throw new Error('Output must be a new directory; existing runs are never overwritten');
  const insideRuntime = relative(webviewDir, output);
  if (!insideRuntime.startsWith('..') && !isAbsolute(insideRuntime)) throw new Error('Output must be outside the runtime directory');
  if (existsSync(join(binaryDir, 'portable.flag'))) throw new Error('Use an instrumented non-portable build');
  for (const file of [join(binaryDir, 'Sotto.exe'), ...libraries.map(name => join(binaryDir, name)), ...crtLibraries.map(name => join(crtDir, name)), nodeExe, join(webviewDir, 'msedgewebview2.exe')]) {
    if (!statSync(file).isFile()) throw new Error(`Missing input: ${file}`);
  }
  if (!readFileSync(join(binaryDir, 'Sotto.exe')).includes(Buffer.from(marker))) {
    throw new Error('The executable is not an instrumented native-e2e build');
  }
  // Enumerate first so a junction cannot map an unrelated host directory.
  filesUnder(webviewDir);
  const input = join(output, 'input');
  const reports = join(output, 'reports');
  mkdirSync(join(input, 'app'), { recursive: true });
  mkdirSync(reports);
  for (const name of ['Sotto.exe', ...libraries]) cpSync(join(binaryDir, name), join(input, 'app', name));
  for (const name of crtLibraries) cpSync(join(crtDir, name), join(input, 'app', name));
  cpSync(nodeExe, join(input, 'node.exe'));
  cpSync(webviewDir, join(input, 'webview'), { recursive: true });
  for (const name of ['bootstrap.ps1', 'probe.mjs', 'webdriver.mjs']) cpSync(join(here, name), join(input, name));
  const manifest = {
    schema: 1,
    kind: 'instrumented-native-experiment',
    createdAt: new Date().toISOString(),
    nodeVersion: process.version,
    webviewRuntime: basename(webviewDir),
    files: Object.fromEntries(filesUnder(input).map(name => [name, sha256(join(input, name))])),
  };
  writeFileSync(join(input, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx' });
  writeFileSync(join(output, 'native-spike.wsb'), sandboxXml(input, reports), { flag: 'wx' });
  return { configuration: join(output, 'native-spike.wsb'), reports, manifest };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [binaryDir, webviewDir, crtDir, output, ...extra] = process.argv.slice(2);
    if (!binaryDir || !webviewDir || !crtDir || !output || extra.length) throw new Error('Usage: node tests/native/package-sandbox.mjs <binary-directory> <WebView2-runtime-directory> <VC143-CRT-directory> <new-output-directory>');
    if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('Windows x64 is required');
    const pinned = readFileSync(join(here, '../../.node-version'), 'utf8').trim();
    if (process.versions.node !== pinned) throw new Error(`Use Node ${pinned}, found ${process.versions.node}`);
    const result = createSandbox({ binaryDir, webviewDir, crtDir, output });
    console.log(`Prepared ${result.configuration}\nReports: ${result.reports}\nNo application was launched.`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
