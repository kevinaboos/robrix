# Makepad patches for the a2app branch

`makepad-6cf59e1-splash-fixes.patch` — fixes and diagnostics for the splash
isolate host, currently applied ONLY as uncommitted edits inside the cargo
git checkout (`~/.cargo/git/checkouts/makepad-*/6cf59e1`). A `cargo update`
or fresh checkout silently discards them, so this patch is the durable copy.

To (re)apply locally:

```sh
cd ~/.cargo/git/checkouts/makepad-*/6cf59e1
git apply /path/to/robrix/a2app/patches/makepad-6cf59e1-splash-fixes.patch
cd /path/to/robrix
cargo clean -p makepad-widgets -p makepad-script && cargo build --features a2app
```

What it contains:

* `widget_tree.rs`: children inserted via `insert_child`/`insert_child_deep`
  get a `manual` flag; a parent refresh keeps live manual children linked
  instead of unlinking (and then deleting) their subtree. This fixes mini-app
  hosts silently losing all `ui.X.render()`/`set_text` effects after the tree
  refreshed their owner (the "second run shows nothing" bug).
* `splash_host.rs`: drain and log script errors after host-bridge callbacks
  (previously they vanished).
* `widget_async.rs`: always-on logs for the previously silent failure paths
  (dropped widget->script calls, empty done deliveries, wrong-VM execution,
  cross-heap call routing), removal of an unrestored `current_vm_id` write in
  `update_global_ui_handle`, and a headless `isolate_tests` regression module
  covering run/force-stop/re-run through the real host bridge.
* `gen_index.rs`: out-of-bounds GenVec access panics with the table's type
  name and index/len, so a cross-heap value names its route.

These belong upstream in makepad (or on a fork branch robrix can pin).
