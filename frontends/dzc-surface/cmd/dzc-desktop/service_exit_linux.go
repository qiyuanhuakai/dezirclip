package main

import "github.com/qiyuanhuakai/dezirclip/clients/go/client"

func observeServiceExit(_ *client.Client) func() { return func() {} }
