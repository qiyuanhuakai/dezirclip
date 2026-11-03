package client

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type desktopEndpoint struct {
	Address     string
	ControlPath string `json:"control_path"`
	PID         uint32
	Instance    string
	Version     string
}

// OpenDesktop discovers the profile using the bundled sibling CLI so portable
// profiles, redirected storage and current-session naming follow Rust exactly.
// On first launch the desktop can opt in to starting its sibling Service.
func OpenDesktop(ctx context.Context, version string, startIfMissing bool) (*Client, error) {
	executable, err := os.Executable()
	if err != nil {
		return nil, err
	}
	executable, err = filepath.EvalSymlinks(executable)
	if err != nil {
		return nil, err
	}
	name := "dzc"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	args := []string{"service", "desktop-endpoint"}
	if startIfMissing {
		args = append(args, "--start-if-missing")
	}
	cmd := exec.CommandContext(ctx, filepath.Join(filepath.Dir(executable), name), args...)
	cmd.SysProcAttr = desktopCLIProcessAttrs()
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	raw, readErr := io.ReadAll(io.LimitReader(stdout, 64*1024+1))
	stdout.Close()
	if err := cmd.Wait(); err != nil {
		return nil, err
	}
	if readErr != nil || len(raw) > 64*1024 {
		return nil, ErrProtocol
	}
	var endpoint desktopEndpoint
	if err := json.Unmarshal(raw, &endpoint); err != nil {
		return nil, ErrProtocol
	}
	return endpoint.connect(ctx, version)
}

func (endpoint desktopEndpoint) connect(ctx context.Context, version string) (*Client, error) {
	if endpoint.PID == 0 || endpoint.Instance == "" || version == "" || endpoint.Version != version ||
		!filepath.IsAbs(endpoint.Address) || !filepath.IsAbs(endpoint.ControlPath) {
		return nil, ErrProtocol
	}
	conn, err := dialLocal(ctx, endpoint.Address, endpoint.PID)
	if err != nil {
		return nil, err
	}
	grant, err := desktopGrant(endpoint.ControlPath)
	if err != nil {
		conn.Close()
		return nil, err
	}
	client, err := handshake(ctx, conn, protocol.HelloParams{ProtocolVersion: protocol.Version,
		ClientVersion: version, ClientRole: protocol.ClientRoleDesktop, Grant: &grant,
		Capabilities: []string{"desktop.extended_methods"}, RequiredCapabilities: []string{"desktop.history.page"}})
	if err != nil {
		return nil, err
	}
	if client.hello.State.ServiceInstanceId != endpoint.Instance || client.hello.CoreVersion != version ||
		!slices.Contains(client.hello.Permissions, protocol.PermissionDesktopHistory) {
		client.Close()
		return nil, ErrProtocol
	}
	client.desktop = &endpoint
	return client, nil
}

// DesktopConnection opens an independent stream to the original authenticated
// Host. Cancelling a search on this stream cannot interrupt an in-flight paste.
func (c *Client) DesktopConnection(ctx context.Context) (*Client, error) {
	if c.desktop == nil {
		return nil, ErrProtocol
	}
	return c.desktop.connect(ctx, c.hello.CoreVersion)
}

// DesktopCall performs one operation on a new authenticated connection.
// Never replay a mutation after an ambiguous transport failure.
func (c *Client) DesktopCall(ctx context.Context, method protocol.Method, params any, schema string, result any) error {
	host, err := c.DesktopConnection(ctx)
	if err != nil {
		return err
	}
	defer host.Close()
	return host.Call(ctx, method, params, schema, result)
}

func desktopGrant(path string) (string, error) {
	file, err := openControlFile(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, 4097))
	defer clear(raw)
	if err != nil || len(raw) > 4096 {
		return "", ErrProtocol
	}
	var credentials struct{ Desktop string }
	if err := json.Unmarshal(raw, &credentials); err != nil || len(credentials.Desktop) != 64 {
		return "", ErrProtocol
	}
	bits, err := hex.DecodeString(credentials.Desktop)
	clear(bits)
	if err != nil {
		return "", ErrProtocol
	}
	return credentials.Desktop, nil
}
