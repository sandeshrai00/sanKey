3. When we want to release a new version - exact steps (nothing to do until you change daemon):
1. Edit manifest.json:version e.g. 0.1.0 -> 0.1.1 and daemon/Cargo.toml:version same
2. git commit -m "bump 0.1.1"
3. git tag v0.1.1 && git push origin main --tags
4. CI release.yml auto checks tag == v + manifest version, builds sorakey-x86_64, writes SHA256SUMS, attests, creates Release v0.1.1 (we just added that check)