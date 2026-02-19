const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const BINARY_NAME = 'masix';
const PACKAGE_BIN_PATH = path.join(__dirname, 'prebuilt', BINARY_NAME);

// Check if running in Termux
const isTermux = process.env.TERMUX_VERSION !== undefined || 
                 process.env.PREFIX === '/data/data/com.termux/files/usr';

if (!isTermux) {
  console.warn('⚠️  @mmmbuto/masix is designed for Android Termux only!');
  console.warn('   Installation may fail on other platforms.');
}

// Check if prebuilt binary exists
const prebuiltPath = path.join(__dirname, 'prebuilt', BINARY_NAME);

if (fs.existsSync(prebuiltPath)) {
  fs.chmodSync(prebuiltPath, 0o755);
  console.log(`✅ Using packaged prebuilt binary: ${prebuiltPath}`);
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

console.log('\n🎉 masix installed successfully!');
console.log('   Run "masix --help" to get started.');
console.log('   Run "masix config init" to create default config.\n');
