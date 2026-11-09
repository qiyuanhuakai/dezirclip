package client

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type desktopEndpoint struct {
	Database    string `json:"database"`
	Address     string
	ControlPath string `json:"control_path"`
	PID         uint32
	Instance    string
	Version     string
}

func (c *Client) DesktopDatabase() (string, error) {
	if c.desktop == nil || !filepath.IsAbs(c.desktop.Database) {
		return "", ErrProtocol
	}
	return c.desktop.Database, nil
}

// StopDesktopService stops only the authenticated standalone Host for this
// profile. Embedded Tauri Hosts and replacement Service instances are retained.
func (c *Client) StopDesktopService(ctx context.Context) error {
	database, err := c.DesktopDatabase()
	if err != nil {
		return err
	}
	var status protocol.ServiceStatus
	if err := c.DesktopCall(ctx, protocol.MethodServiceStatus, protocol.EmptyParams{}, "ServiceStatus", &status); err != nil {
		return err
	}
	if status.Mode != protocol.HostModeService {
		return nil
	}
	cmd, err := bundledCLI(ctx, "service", "stop", "--from-instance", c.hello.State.ServiceInstanceId)
	if err != nil {
		return err
	}
	cmd.Env = append(os.Environ(), "DEZIRCLIP_DB_PATH="+database)
	var stderr desktopStderr
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return stderr.exitError(err)
	}
	return nil
}

func RestartElevatedService(ctx context.Context, instance, database string) (protocol.ServiceStatus, error) {
	var status protocol.ServiceStatus
	output, err := relocationCommand(ctx, "restart-elevated", "--from-instance", instance, "--database", database, "--json")
	if err == nil {
		err = json.Unmarshal(output, &status)
	}
	return status, err
}

type MoveRecovery struct {
	PreviousDirectory string `json:"previous_directory"`
	Target            string `json:"target"`
	Instance          string `json:"instance"`
	NoCapture         bool   `json:"no_capture"`
}

// A successful copy can still need a separate Service cutover.
func MoveActiveProfile(ctx context.Context, directory, instance string, epoch uint64) (*MoveRecovery, error) {
	if !filepath.IsAbs(directory) || instance == "" {
		return nil, ErrProtocol
	}
	output, err := relocationCommand(ctx, "move-data", directory, "--from-instance", instance, "--epoch", strconv.FormatUint(epoch, 10), "--json")
	if err != nil {
		return nil, err
	}
	var result struct {
		Recovery *MoveRecovery `json:"recovery"`
	}
	if err := json.Unmarshal(output, &result); err != nil {
		return nil, err
	}
	return result.Recovery, nil
}

func (r *MoveRecovery) Reconnect(ctx context.Context) error {
	args := []string{"recover-move", r.Target, "--previous-directory", r.PreviousDirectory, "--from-instance", r.Instance}
	if r.NoCapture {
		args = append(args, "--no-capture")
	}
	_, err := relocationCommand(ctx, args...)
	return err
}

func relocationCommand(ctx context.Context, args ...string) ([]byte, error) {
	cmd, err := bundledCLI(ctx, append([]string{"service"}, args...)...)
	if err != nil {
		return nil, err
	}
	output, err := cmd.Output()
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		message := strings.TrimSpace(string(exit.Stderr))
		if message != "" {
			return nil, errors.New(message[:min(len(message), 1024)])
		}
	}
	return output, err
}

func bundledCLI(ctx context.Context, args ...string) (*exec.Cmd, error) {
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
	cmd := exec.CommandContext(ctx, filepath.Join(filepath.Dir(executable), name), args...)
	cmd.SysProcAttr = desktopCLIProcessAttrs()
	return cmd, nil
}

// Drain stderr completely while bounding retained diagnostic data.
type desktopStderr struct {
	data [1024]byte
	n    int
}

func (b *desktopStderr) Write(p []byte) (int, error) {
	b.n += copy(b.data[b.n:], p)
	return len(p), nil
}

func (b *desktopStderr) exitError(err error) error {
	message := strings.TrimSpace(string(b.data[:b.n]))
	if message == "" {
		return fmt.Errorf("desktop-endpoint: %w", err)
	}
	return fmt.Errorf("desktop-endpoint: %w: %s", err, message)
}

// OpenDesktop discovers the profile using the bundled sibling CLI so portable
// profiles, redirected storage and current-session naming follow Rust exactly.
// On first launch the desktop can opt in to starting its sibling Service.
func OpenDesktop(ctx context.Context, version string, startIfMissing bool) (*Client, error) {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	args := []string{"service", "desktop-endpoint"}
	if startIfMissing {
		args = append(args, "--start-if-missing")
	}
	cmd, err := bundledCLI(ctx, args...)
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	var stderr desktopStderr
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	raw, readErr := io.ReadAll(io.LimitReader(stdout, 64*1024+1))
	stdout.Close()
	if err := cmd.Wait(); err != nil {
		return nil, stderr.exitError(err)
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

// RecoverDesktopForPresentation is only for a new explicit window activation.
// It never retries a clipboard operation. A replacement Host requires the UI
// owner to discard its old windows, grants, selections and event cursors.
func (c *Client) RecoverDesktopForPresentation(ctx context.Context) (bool, error) {
	probe, cancel := context.WithTimeout(ctx, 2*time.Second)
	host, err := c.DesktopConnection(probe)
	cancel()
	if err == nil {
		host.Close()
		return false, nil
	}
	database, err := c.DesktopDatabase()
	if err != nil {
		return false, err
	}
	next, err := OpenDesktop(ctx, c.hello.CoreVersion, true)
	if err != nil {
		return false, err
	}
	defer next.Close()
	if next.desktop.Database != database {
		return false, ErrProtocol
	}
	return next.hello.State.ServiceInstanceId != c.hello.State.ServiceInstanceId, nil
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
