# Deziro MyGo Native components

Import the uikit package from this Go module. Create a Kit with a built-in theme name and color mode, call Apply at the start of the view, and use its components with application-owned state and callbacks.

The theme package embeds the colors and widget measurements generated from the corresponding Web styles. From the repository root, use the following commands to verify the generated styles and the native control behavior.

    node scripts/generate-native-ui-themes.mjs --check
    go -C packages/ui-native test ./...

Build the gallery in frontends/dzc-surface/cmd/dzc-ui-gallery. It can create a native window or write a software-rendered PNG without opening a window. The comparison script uses that executable and the browser gallery to check text, component states and foreground and background pixels for each theme and mode.

    go -C frontends/dzc-surface build -o ../../dzc-ui-gallery ./cmd/dzc-ui-gallery
    node scripts/compare-ui-components.mjs native=./dzc-ui-gallery
