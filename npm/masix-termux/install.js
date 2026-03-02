const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const BINARY_NAME = 'masix';
const PACKAGE_BIN_PATH = path.join(__dirname, 'prebuilt', BINARY_NAME);
const PREBUILT_DIR = path.join(__dirname, 'prebuilt');

// Check if running in Termux
const isTermux = process.env.TERMUX_VERSION !== undefined || 
                 process.env.PREFIX === '/data/data/com.termux/files/usr';

if (!isTermux) {
  console.warn('⚠️  @mmmbuto/masix is designed for Android Termux only!');
  console.warn('   Installation may fail on other platforms.');
}

function hasValidElfPrebuilt(binaryPath) {
  if (!fs.existsSync(binaryPath)) return false;
  try {
    const fd = fs.openSync(binaryPath, 'r');
    const buf = Buffer.alloc(4);
    fs.readSync(fd, buf, 0, 4, 0);
    fs.closeSync(fd);
    return buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46; // ELF
  } catch {
    return false;
  }
}

if (hasValidElfPrebuilt(PACKAGE_BIN_PATH)) {
  fs.chmodSync(PACKAGE_BIN_PATH, 0o755);
  console.log(`✅ Using packaged prebuilt binary: ${PACKAGE_BIN_PATH}`);
} else {
  console.log('🔨 No prebuilt binary found. Building from source...');
  console.log('   This requires Rust to be installed in Termux.');
  
  try {
    const masixRoot = path.join(__dirname, '..', '..');
    execSync('cargo build --release', {
      cwd: masixRoot,
      stdio: 'inherit'
    });
    
    let sourceBinary = path.join(masixRoot, 'target', 'release', BINARY_NAME);
    if (!fs.existsSync(sourceBinary)) {
      sourceBinary = path.join(masixRoot, 'target', 'aarch64-linux-android', 'release', BINARY_NAME);
    }
    
    if (fs.existsSync(sourceBinary)) {
      fs.mkdirSync(PREBUILT_DIR, { recursive: true });
      fs.copyFileSync(sourceBinary, PACKAGE_BIN_PATH);
      fs.chmodSync(PACKAGE_BIN_PATH, 0o755);
      console.log(`✅ Binary built and installed at: ${PACKAGE_BIN_PATH}`);
    } else {
      throw new Error('Binary not found after build');
    }
  } catch (error) {
    console.error('❌ Build failed:', error.message);
    console.error('\n📦 Please install Rust in Termux:');
    console.error('   pkg install rust\n');
    process.exit(1);
  }
}

// Run update check in background (don't block install)
console.log('\n🎉 masix installed successfully!');
console.log('   Run "masix --help" to get started.');
console.log('   Run "masix config init" to create default config.');
console.log('   Run "masix check-update" to check for updates.\n');

// Create .masix directory if needed
const masixDir = path.join(require('os').homedir(), '.masix');
if (!fs.existsSync(masixDir)) {
  fs.mkdirSync(masixDir, { recursive: true });
}
