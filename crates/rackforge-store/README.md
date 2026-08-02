# rackforge-store

Maintenance and headless client for RackForge plugin stores. It can create
repository signing keys, sign and verify catalogs, build transportable
`.rfplugin` archives, list signed repositories and install a selected plugin.

Private signing keys belong only on the repository publisher machine. They are
never copied to a RackForge performance device.
