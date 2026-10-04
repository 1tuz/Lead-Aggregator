# macOS duplicate audit — 2026-10-04

Live filesystem, Spotlight and Launch Services inspection. Installed v0.4.5 app and application data preserved. No installer download or full uninstall.

| Check | Before | After |
|---|---:|---:|
| Matching physical app bundles | 2 | 1 |
| Matching Launch Services registrations | 16 | 1 |
| Stale DMG registrations | 14 | 0 |
| Spotlight bundle-ID results | 2 | 1 |

Before physical copies:

- `/Applications/Lead Aggregator.app` (v0.4.5)
- `/Users/antony/Documents/projects/twogis-extractor/target/release/bundle/macos/Lead Aggregator.app`

Before stale DMG registrations:

- `/Volumes/dmg.CDwmXP/Lead Aggregator.app`
- `/Volumes/dmg.lJMUWr/2GIS Extractor.app`
- `/Volumes/dmg.Ii4uMX/Lead Aggregator.app`
- `/Volumes/dmg.K9pS8x/Lead Aggregator.app`
- `/Volumes/dmg.foUSFv/Lead Aggregator.app`
- `/Volumes/dmg.J4VQAj/Lead Aggregator.app`
- `/Volumes/dmg.Tm7xAx/Lead Aggregator.app`
- `/Volumes/dmg.2j6pEz/Lead Aggregator.app`
- `/Volumes/dmg.0bpYaS/Lead Aggregator.app`
- `/Volumes/dmg.1ndlnE/Lead Aggregator.app`
- `/Volumes/dmg.K7OIAM/Lead Aggregator.app`
- `/Volumes/dmg.7nKJct/Lead Aggregator.app`
- `/Volumes/dmg.Bpf597/Lead Aggregator.app`
- `/Volumes/dmg.eXfjrL/Lead Aggregator.app`

After filesystem, Spotlight and Launch Services each contain only `/Applications/Lead Aggregator.app`.

Cleanup validated CFBundleIdentifier before removing the build copy, unregistered current and legacy stale DMG paths, and refreshed Launch Services as the desktop user. Live `lsregister -kill` returned exit 1 with “The -kill option has been removed because it was dangerous and no longer useful.” Used supported `lsregister -gc`, `lsregister -r -apps local,system,user`, then `lsregister -f '/Applications/Lead Aggregator.app'`. No reboot/database deletion needed. Updated scripts choose legacy reset only when advertised by help; otherwise they use GC and rescan.

Discovery covers registered bundle IDs, Spotlight bundle-ID lookup, `/Applications`, `~/Applications`, current working directory `target`, custom install directory, and mounted `/Volumes`. Arbitrary unindexed/unregistered apps outside these roots are not globally scanned. Installer normalizes custom directories and preserved paths; identity validation protects unrelated same-name apps. Installer detaches its DMG before refreshing the registry. Busy images cause a visible failure, and mounted temporary directories are retained safely.

Validation: `bash scripts/tests/test_macos_duplicates.sh`, `bash scripts/tests/test_install_pipe.sh`, `bash scripts/tests/test_macos_paths.sh` passed. Duplicate tests cover stale current/legacy registrations, relative preserved paths, unrelated apps, install/uninstall cleanup, legacy/modern registry refresh, and mounted/busy DMGs. Live after-state verified separately with `lsregister -dump` and `mdfind`.
