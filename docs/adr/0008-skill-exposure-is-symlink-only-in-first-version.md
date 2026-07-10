# Skill Exposure is symlink-only in the first version

The first version of Skill management exposes Shared Skills to Agent Clients only by creating directory symlinks from writable Skill Locations to the canonical Skill Library directory. This keeps sharing real-time and single-source, and deliberately avoids copy synchronization, generated wrappers, drift detection, and client-specific manifest adapters.
