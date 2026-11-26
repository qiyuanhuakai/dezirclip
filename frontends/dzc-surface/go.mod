module github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface

go 1.27.3

require (
	github.com/MoeForge/moego v0.1.0-alpha.4
	github.com/ebitengine/purego v0.11.1
	github.com/egoist/mygo v0.10.0
	github.com/qiyuanhuakai/dezirclip/clients/go v0.0.0
	github.com/qiyuanhuakai/deziro-ui/packages/ui-native v0.4.1
	github.com/qiyuanhuakai/deziro-ui/packages/ui-presets v0.4.1
	golang.org/x/crypto v0.57.0
	golang.org/x/sys v0.49.0
	golang.org/x/text v0.43.0
)

require (
	github.com/Microsoft/go-winio v0.6.4 // indirect
	github.com/go-text/typesetting v0.3.5 // indirect
	github.com/santhosh-tekuri/jsonschema/v6 v6.0.3 // indirect
	github.com/tdewolff/parse/v2 v2.25.0 // indirect
	golang.org/x/image v0.46.0 // indirect
	golang.org/x/net v0.59.0 // indirect
)

replace github.com/qiyuanhuakai/dezirclip/clients/go => ../../clients/go
