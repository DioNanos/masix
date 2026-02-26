# Homebrew Distribution

MasiX can be distributed on macOS/Linux through a Homebrew tap, while Termux keeps using npm (`@mmmbuto/masix`).

## 1. Create tap repository

Create a dedicated repository:

- `DioNanos/homebrew-masix`

Structure:

```text
homebrew-masix/
  Formula/
    masix.rb
```

## 2. Generate/update formula

From the main MasiX repo, after a GitHub release tag is available:

```bash
./scripts/generate_homebrew_formula.sh <version> ~/Dev/homebrew-masix
```

This script:

- downloads source tarball from GitHub
- computes sha256
- writes `Formula/masix.rb`

Example for 0.2.5:

```bash
./scripts/generate_homebrew_formula.sh 0.2.5 ~/Dev/homebrew-masix
```

## 3. Publish tap

In `homebrew-masix`:

```bash
git add Formula/masix.rb
git commit -m "masix <version>"
git push
```

## 4. User install

```bash
brew tap DioNanos/masix
brew install masix
```

Upgrade:

```bash
brew update
brew upgrade masix
```

## Notes

- Mobile-specific runtime features (Termux boot/wake, SMS watcher, Android intent tool) are disabled outside Termux.
- Homebrew formula builds `crates/masix-cli` from source with Rust.
