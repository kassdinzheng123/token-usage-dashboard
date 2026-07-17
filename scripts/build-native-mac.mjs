import { copyFile, chmod, cp, mkdir, rm, stat, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const nativeRoot = join(repoRoot, 'macos', 'TokenUsageNative');
const productName = 'Token Usage';
const version = '1.0.1';
const appId = 'com.tokenusage.dashboard.native';
const minimumMacOSVersion = '14.0';
const appPath = join(repoRoot, 'release', 'native', `${productName}.app`);
const dmgPath = join(repoRoot, 'release', 'native', `${productName}-${version}-native-${process.arch}.dmg`);
const contentsPath = join(appPath, 'Contents');
const macOSPath = join(contentsPath, 'MacOS');
const resourcesPath = join(contentsPath, 'Resources');
const backendPath = join(resourcesPath, 'Backend');
const swiftResourceBundleName = 'TokenUsageNative_TokenUsageNative.bundle';
const shouldCreateDmg = process.argv.includes('--dmg');
const shouldNotarize = process.argv.includes('--notarize') || process.env.NOTARIZE === '1';
const homeDir = process.env.HOME || '';
const rustPathRemaps = [
  `${repoRoot}=.`,
  homeDir ? `${join(homeDir, '.cargo', 'registry')}=/cargo/registry` : null,
  homeDir ? `${join(homeDir, '.cargo', 'git', 'checkouts')}=/cargo/git/checkouts` : null,
].filter(Boolean);
const rustFlags = [
  process.env.RUSTFLAGS,
  ...rustPathRemaps.map((mapping) => `--remap-path-prefix=${mapping}`),
].filter(Boolean).join(' ');

function run(command, args, options = {}) {
  const logArgs = options.logArgs ?? args;
  console.log(`$ ${[command, ...logArgs].join(' ')}`);
  execFileSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    stdio: 'inherit',
    env: { ...process.env, ...(options.env ?? {}) },
  });
}

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

async function copyRustBackendBinary() {
  const source = join(repoRoot, 'rust-backend', 'target', 'release', 'token-usage-server');
  const target = join(backendPath, 'token-usage-server');

  if (!await exists(source)) {
    throw new Error(`Rust backend binary is missing: ${source}`);
  }

  await copyFile(source, target);
  await chmod(target, 0o755);
  run('strip', ['-S', target]);
}

async function copySwiftResourceBundle() {
  const source = join(nativeRoot, '.build', 'release', swiftResourceBundleName);
  const resourcesTarget = join(resourcesPath, swiftResourceBundleName);

  if (!await exists(source)) {
    throw new Error(`Swift resource bundle is missing: ${source}`);
  }

  await cp(source, resourcesTarget, { recursive: true });
}

async function writeInfoPlist() {
  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>TokenUsageNative</string>
  <key>CFBundleIdentifier</key>
  <string>${appId}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${productName}</string>
  <key>CFBundleDisplayName</key>
  <string>${productName}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>LSMinimumSystemVersion</key>
  <string>${minimumMacOSVersion}</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>Token Usage starts a local server on 127.0.0.1 to read usage records.</string>
</dict>
</plist>
`;

  await writeFile(join(contentsPath, 'Info.plist'), plist);
}

function signingIdentity() {
  return process.env.CODESIGN_IDENTITY || '-';
}

function signAppIfRequested() {
  if (process.env.SKIP_CODESIGN === '1') {
    if (shouldNotarize) {
      throw new Error('Cannot notarize when SKIP_CODESIGN=1.');
    }
    console.warn('[warn] App signing skipped because SKIP_CODESIGN=1.');
    return;
  }

  const identity = signingIdentity();
  if (shouldNotarize && identity === '-') {
    throw new Error('CODESIGN_IDENTITY must be set to a Developer ID Application identity when notarizing.');
  }

  run('codesign', [
    '--force',
    '--options',
    'runtime',
    '--sign',
    identity,
    join(backendPath, 'token-usage-server'),
  ]);
  run('codesign', [
    '--force',
    '--deep',
    '--options',
    'runtime',
    '--sign',
    identity || '-',
    appPath,
  ]);
}

function notarySubmitConfig() {
  if (process.env.NOTARY_PROFILE) {
    const args = ['--keychain-profile', process.env.NOTARY_PROFILE];
    return { args, logArgs: args };
  }

  const appleId = process.env.APPLE_ID;
  const teamId = process.env.APPLE_TEAM_ID;
  const password = process.env.APPLE_APP_SPECIFIC_PASSWORD;
  if (appleId && teamId && password) {
    return {
      args: ['--apple-id', appleId, '--team-id', teamId, '--password', password],
      logArgs: ['--apple-id', appleId, '--team-id', teamId, '--password', '<redacted>'],
    };
  }

  throw new Error(
    'Notarization credentials are missing. Set NOTARY_PROFILE, or APPLE_ID, APPLE_TEAM_ID, and APPLE_APP_SPECIFIC_PASSWORD.'
  );
}

function validateNotarizationConfiguration() {
  if (!shouldNotarize) {
    return;
  }

  if (!shouldCreateDmg) {
    throw new Error('Notarization requires --dmg.');
  }

  const identity = signingIdentity();
  if (identity === '-') {
    throw new Error('CODESIGN_IDENTITY must be set to a Developer ID Application identity when notarizing.');
  }

  notarySubmitConfig();
}

function createDmgIfRequested() {
  if (!shouldCreateDmg) {
    return;
  }

  run('hdiutil', [
    'create',
    '-volname',
    productName,
    '-srcfolder',
    appPath,
    '-ov',
    '-format',
    'UDZO',
    dmgPath,
  ]);
  run('du', ['-sh', dmgPath]);
}

function signDmgIfRequested() {
  if (!shouldCreateDmg || process.env.SKIP_CODESIGN === '1') {
    return;
  }

  const identity = signingIdentity();
  if (identity === '-') {
    return;
  }

  run('codesign', ['--force', '--sign', identity, dmgPath]);
}

function notarizeDmgIfRequested() {
  if (!shouldNotarize) {
    return;
  }

  const submitConfig = notarySubmitConfig();
  run('xcrun', ['notarytool', 'submit', dmgPath, '--wait', ...submitConfig.args], {
    logArgs: ['notarytool', 'submit', dmgPath, '--wait', ...submitConfig.logArgs],
  });
  run('xcrun', ['stapler', 'staple', dmgPath]);
  run('spctl', ['-a', '-vvv', '-t', 'open', dmgPath]);
}

async function main() {
  validateNotarizationConfiguration();

  run('cargo', ['build', '--release', '--manifest-path', 'rust-backend/Cargo.toml'], {
    env: { RUSTFLAGS: rustFlags },
  });
  await rm(join(nativeRoot, '.build', 'release', swiftResourceBundleName), { recursive: true, force: true });
  run('swift', ['build', '-c', 'release', '--disable-sandbox'], { cwd: nativeRoot });

  await rm(appPath, { recursive: true, force: true });
  await mkdir(macOSPath, { recursive: true });
  await mkdir(backendPath, { recursive: true });

  await copyFile(
    join(nativeRoot, '.build', 'release', 'TokenUsageNative'),
    join(macOSPath, 'TokenUsageNative')
  );
  await chmod(join(macOSPath, 'TokenUsageNative'), 0o755);
  run('strip', ['-S', join(macOSPath, 'TokenUsageNative')]);
  await copySwiftResourceBundle();

  await writeInfoPlist();
  await copyFile(join(repoRoot, 'assets', 'icon.icns'), join(resourcesPath, 'AppIcon.icns'));

  await copyRustBackendBinary();

  console.log(`Built ${appPath}`);
  run('du', ['-sh', appPath]);
  signAppIfRequested();
  createDmgIfRequested();
  signDmgIfRequested();
  notarizeDmgIfRequested();
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
