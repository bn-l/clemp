# Release Guide for clemp

Steps to create a new release and update the Homebrew tap.

## 1. Commit pending changes

Ensure all feature/fix commits are done before bumping version.

## 2. Bump version in Cargo.toml

```bash
# Edit Cargo.toml, update version field (e.g., 0.1.4 -> 0.1.5)
```

## 3. Build to update Cargo.lock

```bash
cargo build --release
```

## 4. Commit version bump

```bash
git add Cargo.toml Cargo.lock && git commit -m "hk: bump version to 0.1.5"
```

## 5. Push and tag

```bash
git push origin master
git tag v0.1.5 && git push origin v0.1.5
```

## 6. Get SHA256 of release tarball

```bash
curl -sL https://github.com/bn-l/clemp/archive/refs/tags/v0.1.5.tar.gz | shasum -a 256
```

## 7. Update Homebrew formula

Edit `../clemp-tap/Formula/clemp.rb`:
- Update `url` to new tag
- Update `sha256` to value from step 6

## 8. Push tap update

```bash
cd ../clemp-tap
git add Formula/clemp.rb && git commit -m "clemp 0.1.5" && git push origin main
```

## 9. Verify installation

```bash
brew update
brew upgrade clemp
clemp --version  # Should show new version
```
