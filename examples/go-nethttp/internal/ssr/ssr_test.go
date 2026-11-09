package ssr

import (
	"context"
	"errors"
	"os"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"
)

// The renderer is shared by the tests: compiling the module takes seconds.
var (
	shared    *Renderer
	sharedErr error
	once      sync.Once
)

func renderer(t *testing.T) *Renderer {
	t.Helper()
	path := os.Getenv("SSR_WASM_PATH")
	if path == "" {
		path = "../../../../packages/components/dist/ssr-wasm/index.wasm"
	}
	if _, err := os.Stat(path); err != nil {
		t.Skipf("plugin not found at %s (set SSR_WASM_PATH): %v", path, err)
	}
	once.Do(func() {
		shared, sharedErr = New(context.Background(), path, 2)
	})
	if sharedErr != nil {
		t.Fatal(sharedErr)
	}
	return shared
}

var sid = regexp.MustCompile(`\ss-id="(\d+)"`)

func ids(html string) []string {
	var out []string
	for _, m := range sid.FindAllStringSubmatch(html, -1) {
		out = append(out, m[1])
	}
	return out
}

func TestFragmentRender(t *testing.T) {
	h, err := renderer(t).Acquire(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer h.Release()

	out := h.Render(`<demo-counter start="5" step="2"></demo-counter>`, Fragment)
	if !strings.Contains(out, `<template shadowrootmode="open">`) {
		t.Fatalf("expected declarative shadow DOM, got %q", out)
	}
	if strings.Contains(out, "<html") {
		t.Fatalf("fragment render should not produce a document, got %q", out)
	}
}

// Fragment ids are reset once per Acquire, before the first fragment: fragments in one request
// continue the id sequence, and the next request starts again from 1.
func TestFragmentIdsResetOncePerAcquire(t *testing.T) {
	r := renderer(t)
	frag := `<demo-badge tone="success">SSR</demo-badge>`
	for range 2 {
		h, err := r.Acquire(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		a := ids(h.Render(frag, Fragment))
		b := ids(h.Render(frag, Fragment))
		h.Release()
		if len(a) != 1 || len(b) != 1 || a[0] != "1" || b[0] != "2" {
			t.Fatalf("want ids [1] then [2], got %v then %v", a, b)
		}
	}
}

func TestParseResult(t *testing.T) {
	if html, err := parseResult([]byte(`{"html":"<p>ok</p>","hydratedCount":0}`), "in"); err != nil || html != "<p>ok</p>" {
		t.Fatalf("got %q, %v", html, err)
	}
	for _, bad := range []string{`{"__error":"ReferenceError: x"}`, `{"__error":{"message":"x"}}`, `{}`, `not json`} {
		if html, err := parseResult([]byte(bad), "in"); err == nil || html != "in" {
			t.Errorf("%s: want an error and the input back, got %q, %v", bad, html, err)
		}
	}
}

func TestAcquireWaitsForAFreeInstance(t *testing.T) {
	r := renderer(t)
	var held []*Handle
	for range cap(r.pool) {
		h, err := r.Acquire(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		held = append(held, h)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if _, err := r.Acquire(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("want deadline exceeded with the pool empty, got %v", err)
	}
	for _, h := range held {
		h.Release()
	}
	if len(r.pool) != cap(r.pool) {
		t.Fatalf("pool has %d of %d instances after release", len(r.pool), cap(r.pool))
	}
}

// Full-document renders number their ids from 1 by themselves, without any reset.
func TestDocumentIdsStartFromOne(t *testing.T) {
	r := renderer(t)
	page := `<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>`
	h, err := r.Acquire(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer h.Release()
	for range 3 {
		if got := ids(h.Render(page, nil)); len(got) != 2 || got[0] != "1" || got[1] != "2" {
			t.Fatalf("want ids [1 2], got %v", got)
		}
	}
}
