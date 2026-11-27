package application

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func ProfileInfo(ctx context.Context, host *client.Client) (protocol.DesktopProfileInfo, error) {
	var profile protocol.DesktopProfileInfo
	err := host.DesktopCall(ctx, protocol.MethodDesktopProfileInfo, protocol.ToolCatalogParams{}, "DesktopProfileInfo", &profile)
	return profile, err
}

func OpenDataFolder(ctx context.Context, host *client.Client, openPath func(string) error) error {
	profile, err := ProfileInfo(ctx, host)
	if err != nil {
		return err
	}
	return openPath(profile.DataDirectory)
}
