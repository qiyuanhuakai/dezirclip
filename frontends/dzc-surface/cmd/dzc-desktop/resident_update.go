package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"time"

	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

const nativeUpdateStartupDelay = 15 * time.Second

func checkResidentUpdate(ctx context.Context, database, channel string, automatic bool) (*nativeUpdateCheck, error) {
	due, err := recordNativeUpdateAttempt(database, channel, automatic, time.Now())
	if automatic {
		if err != nil {
			return nil, errUpdateSchedule
		}
		if !due {
			return nil, nil
		}
	}
	raw, err := checkDesktopUpdate(ctx, channel)
	if err != nil {
		return nil, err
	}
	result, err := parseNativeUpdate(raw, channel)
	if err != nil {
		return nil, err
	}
	return &nativeUpdateCheck{Result: result, Checked: time.Now()}, nil
}

func residentUpdateArtifact(check *nativeUpdateCheck, channel, version string) (mygoArtifact, error) {
	if !check.validFor(channel, buildinfo.Version, time.Now()) || check.Result.Status != "available" ||
		check.Result.Available == nil || check.Result.Available.Version != version ||
		check.Result.Available.Artifact == nil {
		return mygoArtifact{}, errors.New("update_invalid_manifest")
	}
	return *check.Result.Available.Artifact, nil
}

func downloadResidentUpdate(ctx context.Context, artifact mygoArtifact, version, target string, progress func(nativeInstallProgress)) (mygoInstallRequest, error) {
	ctx, cancel := context.WithTimeout(ctx, 12*time.Minute)
	defer cancel()
	path, err := downloadMyGoUpdate(ctx, artifact, version, target, func(downloaded, total int64) {
		progress(nativeInstallProgress{Phase: updateDownloading, Downloaded: downloaded, Total: total})
	})
	if err != nil {
		return mygoInstallRequest{}, err
	}
	request := mygoInstallRequest{Path: path, Version: version, Signature: artifact.Signature}
	if err := request.validate(); err != nil {
		discardResidentUpdate(path)
		return mygoInstallRequest{}, err
	}
	return request, nil
}

func discardResidentUpdate(path string) {
	_ = os.Remove(path)
	_ = os.Remove(filepath.Dir(path))
}
