#!/usr/bin/env node

const https = require('https');
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const PACKAGE_NAME = '@mmmbuto/masix';
const CACHE_FILE = path.join(require('os').homedir(), '.masix', '.update-check');
const CACHE_DURATION = 24 * 60 * 60 * 1000; // 24 hours

function getCurrentVersion() {
  try {
    const pkgPath = path.join(__dirname, '..', 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    return pkg.version;
  } catch {
    return 'unknown';
  }
}

function getCacheDir() {
  const cacheDir = path.join(require('os').homedir(), '.masix');
  if (!fs.existsSync(cacheDir)) {
    fs.mkdirSync(cacheDir, { recursive: true });
  }
  return cacheDir;
}

function getCachedUpdate() {
  try {
    if (fs.existsSync(CACHE_FILE)) {
      const cached = JSON.parse(fs.readFileSync(CACHE_FILE, 'utf8'));
      const now = Date.now();
      if (now - cached.timestamp < CACHE_DURATION) {
        return cached;
      }
    }
  } catch {}
  return null;
}

function setCachedUpdate(latestVersion) {
  try {
    fs.writeFileSync(CACHE_FILE, JSON.stringify({
      latestVersion,
      timestamp: Date.now()
    }));
  } catch {}
}

function fetchLatestVersion() {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: 'registry.npmjs.org',
      path: `/${PACKAGE_NAME}/latest`,
      method: 'GET',
      headers: {
        'Accept': 'application/json'
      },
      timeout: 5000
    };

    const req = https.request(options, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try {
          const pkg = JSON.parse(data);
          resolve(pkg.version);
        } catch {
          reject(new Error('Invalid response'));
        }
      });
    });

    req.on('error', reject);
    req.on('timeout', () => {
      req.destroy();
      reject(new Error('Timeout'));
    });
    req.end();
  });
}

function compareVersions(v1, v2) {
  const parts1 = v1.replace(/[^0-9.]/g, '').split('.').map(Number);
  const parts2 = v2.replace(/[^0-9.]/g, '').split('.').map(Number);
  
  for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
    const p1 = parts1[i] || 0;
    const p2 = parts2[i] || 0;
    if (p1 < p2) return -1;
    if (p1 > p2) return 1;
  }
  return 0;
}

async function checkForUpdate(options = {}) {
  const { quiet = false, force = false } = options;
  const currentVersion = getCurrentVersion();
  
  if (currentVersion === 'unknown') {
    if (!quiet) console.log('Unable to determine current version');
    return { hasUpdate: false, current: currentVersion, latest: 'unknown' };
  }

  // Check cache first (unless forced)
  if (!force) {
    const cached = getCachedUpdate();
    if (cached && cached.latestVersion) {
      const hasUpdate = compareVersions(currentVersion, cached.latestVersion) < 0;
      return {
        hasUpdate,
        current: currentVersion,
        latest: cached.latestVersion,
        cached: true
      };
    }
  }

  // Fetch from npm registry
  try {
    const latestVersion = await fetchLatestVersion();
    setCachedUpdate(latestVersion);
    
    const hasUpdate = compareVersions(currentVersion, latestVersion) < 0;
    
    return {
      hasUpdate,
      current: currentVersion,
      latest: latestVersion,
      cached: false
    };
  } catch (error) {
    if (!quiet) {
      console.log(`Unable to check for updates: ${error.message}`);
    }
    return {
      hasUpdate: false,
      current: currentVersion,
      latest: currentVersion,
      error: error.message
    };
  }
}

function printUpdateMessage(current, latest) {
  console.log('\n┌─────────────────────────────────────────────┐');
  console.log('│  📦 Update Available!                        │');
  console.log('├─────────────────────────────────────────────┤');
  console.log(`│  Current: v${current.padEnd(28)} │`);
  console.log(`│  Latest:  v${latest.padEnd(28)} │`);
  console.log('├─────────────────────────────────────────────┤');
  console.log('│  Run to update:                              │');
  console.log('│  npm install -g @mmmbuto/masix@latest       │');
  console.log('└─────────────────────────────────────────────┘\n');
}

async function main() {
  const args = process.argv.slice(2);
  const quiet = args.includes('--quiet') || args.includes('-q');
  const force = args.includes('--force') || args.includes('-f');
  const json = args.includes('--json');
  
  const result = await checkForUpdate({ quiet, force });
  
  if (json) {
    console.log(JSON.stringify(result));
    return;
  }
  
  if (result.hasUpdate) {
    printUpdateMessage(result.current, result.latest);
    process.exit(1); // Exit code 1 indicates update available
  } else {
    if (!quiet) {
      console.log(`✅ masix is up to date (v${result.current})`);
    }
    process.exit(0);
  }
}

// Export for use as module
module.exports = { checkForUpdate, getCurrentVersion, compareVersions };

// Run if called directly
if (require.main === module) {
  main();
}
