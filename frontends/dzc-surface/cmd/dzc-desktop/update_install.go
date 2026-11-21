package main

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"

	"golang.org/x/crypto/blake2b"
)

// Populated from the same public key as Tauri by the release build.
var nativeUpdaterPublicKey string
var updateVersion = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$`)
var errUpdateVerification = errors.New("update_download_or_signature_failed")
var errUpdateInstall = errors.New("update_install_failed")

type mygoArtifact struct {
	URL, Signature string
}

func mygoUpdateName(version string) string {
	if !updateVersion.MatchString(version) {
		return ""
	}
	switch runtime.GOOS + "/" + runtime.GOARCH {
	case "windows/amd64":
		return fmt.Sprintf("DezirClip-v%s-windows-x64-MyGo-setup.exe", version)
	case "windows/arm64":
		return fmt.Sprintf("DezirClip-v%s-windows-arm64-MyGo-setup.exe", version)
	case "linux/amd64":
		return fmt.Sprintf("DezirClip-MyGo_%s_amd64.AppImage", version)
	case "linux/arm64":
		return fmt.Sprintf("DezirClip-MyGo_%s_aarch64.AppImage", version)
	default:
		return ""
	}
}

func verifyMyGoUpdate(reader io.Reader, encodedKey, encodedSignature, version, filename string) error {
	decode := func(text string) []byte {
		data, err := base64.StdEncoding.Strict().DecodeString(strings.TrimSpace(text))
		if err != nil {
			return nil
		}
		return data
	}
	keyLines := strings.Split(strings.TrimSpace(string(decode(encodedKey))), "\n")
	signatureLines := strings.Split(strings.TrimSpace(string(decode(encodedSignature))), "\n")
	if len(keyLines) != 2 || len(signatureLines) != 4 {
		return errUpdateVerification
	}
	key, signature, global := decode(keyLines[1]), decode(signatureLines[1]), decode(signatureLines[3])
	comment, trusted := strings.CutPrefix(strings.TrimSpace(signatureLines[2]), "trusted comment: ")
	if len(key) != 42 || string(key[:2]) != "Ed" || len(signature) != 74 || string(signature[:2]) != "ED" ||
		!bytes.Equal(key[2:10], signature[2:10]) || len(global) != ed25519.SignatureSize || !trusted {
		return errUpdateVerification
	}
	var signedVersion, signedFile string
	for _, field := range strings.Split(comment, "\t") {
		switch {
		case strings.HasPrefix(field, "version:") && signedVersion == "":
			signedVersion = strings.TrimPrefix(field, "version:")
		case strings.HasPrefix(field, "file:") && signedFile == "":
			signedFile = strings.TrimPrefix(field, "file:")
		case strings.HasPrefix(field, "version:") || strings.HasPrefix(field, "file:"):
			return errUpdateVerification
		}
	}
	if strings.TrimPrefix(signedVersion, "v") != version || signedFile != filename {
		return errUpdateVerification
	}
	publicKey := ed25519.PublicKey(key[10:])
	if !ed25519.Verify(publicKey, append(append([]byte(nil), signature[10:]...), comment...), global) {
		return errUpdateVerification
	}
	digest, _ := blake2b.New512(nil)
	if _, err := io.Copy(digest, reader); err != nil || !ed25519.Verify(publicKey, digest.Sum(nil), signature[10:]) {
		return errUpdateVerification
	}
	return nil
}

const maxUpdateBytes = 512 * 1024 * 1024

func downloadMyGoUpdate(ctx context.Context, artifact mygoArtifact, version, target string, report func(int64, int64)) (string, error) {
	filename := mygoUpdateName(version)
	endpoint, err := url.Parse(artifact.URL)
	if err != nil || filename == "" || endpoint.Scheme != "https" || endpoint.Host != "github.com" || endpoint.User != nil ||
		endpoint.Path != "/qiyuanhuakai/dezirclip/releases/download/v"+version+"/"+filename ||
		endpoint.RawQuery != "" || endpoint.Fragment != "" || nativeUpdaterPublicKey == "" {
		return "", errUpdateVerification
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return "", errUpdateVerification
	}
	httpClient := &http.Client{Timeout: 12 * time.Minute, CheckRedirect: updateRedirect}
	response, err := httpClient.Do(req)
	if err != nil {
		return "", errUpdateVerification
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK || response.ContentLength > maxUpdateBytes {
		return "", errUpdateVerification
	}
	parent := ""
	if runtime.GOOS == "linux" {
		parent = filepath.Dir(target)
	}
	staging, err := os.MkdirTemp(parent, ".dezirclip-mygo-update-")
	if err != nil {
		return "", err
	}
	finished := false
	defer func() {
		if !finished {
			_ = os.Remove(filepath.Join(staging, filename))
			_ = os.Remove(staging)
		}
	}()
	path := filepath.Join(staging, filename)
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o700)
	if err != nil {
		return "", err
	}
	defer file.Close()
	if err := copyUpdateBody(file, response.Body, response.ContentLength, report); err != nil {
		return "", err
	}
	if _, err = file.Seek(0, io.SeekStart); err != nil {
		return "", errUpdateVerification
	}
	if err = verifyMyGoUpdate(file, nativeUpdaterPublicKey, artifact.Signature, version, filename); err != nil {
		return "", err
	}
	if err = file.Sync(); err != nil {
		return "", err
	}
	finished = true
	return path, nil
}

type updateDownloadProgress struct {
	total, written, reported int64
	last                     time.Time
	report                   func(int64, int64)
}

func (p *updateDownloadProgress) Write(buffer []byte) (int, error) {
	p.written += int64(len(buffer))
	if time.Since(p.last) >= 100*time.Millisecond {
		p.emit()
	}
	return len(buffer), nil
}

func (p *updateDownloadProgress) emit() {
	p.last, p.reported = time.Now(), p.written
	p.report(p.written, p.total)
}

func copyUpdateBody(target io.Writer, body io.Reader, total int64, report func(int64, int64)) error {
	var progress *updateDownloadProgress
	if report != nil {
		progress = &updateDownloadProgress{total: total, report: report}
		progress.emit()
		target = io.MultiWriter(target, progress)
	}
	size, err := io.Copy(target, io.LimitReader(body, maxUpdateBytes+1))
	if err != nil || size == 0 || size > maxUpdateBytes || (total >= 0 && size != total) {
		return errUpdateVerification
	}
	if progress != nil && progress.reported != size {
		progress.emit()
	}
	return nil
}

type mygoInstallRequest struct {
	Path, Version, Signature string
}

type mygoUpdateHandoff struct {
	finish        func() error
	abort         func()
	reportFailure func(string)
}

func (r mygoInstallRequest) validate() error {
	filename := mygoUpdateName(r.Version)
	if filename == "" || nativeUpdaterPublicKey == "" || !filepath.IsAbs(r.Path) || filepath.Base(r.Path) != filename ||
		!strings.HasPrefix(filepath.Base(filepath.Dir(r.Path)), ".dezirclip-mygo-update-") {
		return errUpdateVerification
	}
	info, err := os.Lstat(r.Path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > maxUpdateBytes {
		return errUpdateVerification
	}
	file, err := os.Open(r.Path)
	if err != nil {
		return errUpdateVerification
	}
	defer file.Close()
	return verifyMyGoUpdate(file, nativeUpdaterPublicKey, r.Signature, r.Version, filename)
}
