# rackforge-store

Maintenance and headless client for RackForge plugin stores. It can create
repository signing keys, sign and verify catalogs, build transportable
`.rfplugin` archives, list signed repositories, install a selected plugin, and
report and install updates. `official` stands for RackForge's own catalog
wherever a repositories file is expected; see
docs/architecture/plugin-repositories.md.

Private signing keys belong only on the repository publisher machine. They are
never copied to a RackForge performance device.
