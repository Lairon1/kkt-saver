# Local patch

This directory contains upstream eframe 0.33.3. The only source change is in
`src/native/run.rs`: `check_redraw_requests` now sets `ControlFlow::Wait` when
no repaint deadline remains. Without this reset, the Windows event loop can
stay in `Poll` after a hidden window consumes its last repaint deadline and
spin while the application sits in the tray.

Remove this patch after migrating the application to eframe 0.36.2 or newer,
which includes the upstream idle-loop fix. The upstream MIT and Apache-2.0
license texts are included alongside this file.
