# Deziro MyGo Native components

Import the uikit package from this MIT Go module. Set the Kit's Mode to light or dark, call Apply at the start of the view, and use its components with application-owned state and callbacks. NewKit accepts a caller-supplied palette and widget styles.

The separate GPL ui-presets module contains six palettes and widget measurements generated from the corresponding Web styles. From the repository root, use the following commands to verify the generated styles and the native control behavior.

    node scripts/generate-native-ui-themes.mjs --check
    go -C packages/ui-native test ./...
    go -C packages/ui-presets test ./...

Build the gallery in frontends/dzc-surface/cmd/dzc-ui-gallery. It can create a native window or write a software-rendered PNG without opening a window. The comparison script uses that executable and the browser gallery to check text, component states and foreground and background pixels for each theme and mode.

    go -C frontends/dzc-surface build -o ../../dzc-ui-gallery ./cmd/dzc-ui-gallery
    node scripts/compare-ui-components.mjs native=./dzc-ui-gallery

See the [WebView and MyGo Native comparisons](../ui-react/README.md#webview-and-mygo-native-comparisons) for settings toggles, sliders, shortcut bindings, scroll-to-top buttons, and emoji controls.
