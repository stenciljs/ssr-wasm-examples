package main

import (
	"bytes"
	"embed"
	"encoding/json"
	"html/template"
	"log/slog"
	"net/http"
	"strings"

	"ssr-wasm-examples/go-nethttp/internal/ssr"
)

const framework = "Go net/http"

//go:embed templates/*.html
var templateFS embed.FS

type task struct {
	Title string `json:"title"`
	Done  *bool  `json:"done,omitempty"` // the third task has no "done" key at all
}

func ptr[T any](v T) *T { return &v }

var tasks = []task{
	{Title: "Write components", Done: ptr(true)},
	{Title: `Render with <wasm> & "friends"`, Done: ptr(false)},
	{Title: "Hydrate on the client"},
}

// tasksJSON is the task list as JSON. It's a plain string, not template.HTMLAttr, so
// html/template escapes it for the attribute it's written into. HTML escaping is the
// template's job, so encoding/json's own <-style escaping is switched off.
var tasksJSON = func() string {
	var b strings.Builder
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(tasks); err != nil {
		panic(err)
	}
	return strings.TrimSuffix(b.String(), "\n")
}()

type pageData struct {
	Framework string
	Tasks     string
	SSR       *ssr.Handle // helper mode only
}

// Two template sets sharing the layout and component templates; each adds its own "main".
var (
	indexTmpl  = template.Must(template.ParseFS(templateFS, "templates/layout.html", "templates/components.html", "templates/index.html"))
	helperTmpl = newHelperTemplates()
)

// newHelperTemplates builds the helper-mode set with its `stencil` func:
//
//	{{stencil .SSR "demo-card" .}}
//
// executes the named component template (so html/template does all the contextual escaping),
// renders the result as a fragment with the request's plugin handle, and returns it as
// template.HTML so it's inlined as-is.
func newHelperTemplates() *template.Template {
	var t *template.Template
	stencil := func(h *ssr.Handle, name string, data any) (template.HTML, error) {
		var b bytes.Buffer
		if err := t.ExecuteTemplate(&b, name, data); err != nil {
			return "", err
		}
		return template.HTML(h.Render(b.String(), ssr.Fragment)), nil
	}
	t = template.Must(template.New("").
		Funcs(template.FuncMap{"stencil": stencil}).
		ParseFS(templateFS, "templates/layout.html", "templates/components.html", "templates/helper.html"))
	return t
}

// indexHandler renders the page with no SSR; the middleware wrapped around it does that.
func indexHandler(w http.ResponseWriter, r *http.Request) {
	writePage(w, r, indexTmpl, pageData{Framework: framework, Tasks: tasksJSON})
}

// helperHandler takes one plugin instance for the request and lets every `stencil` call in the
// template share it (and its hydration-id sequence, reset before the first fragment).
func helperHandler(renderer *ssr.Renderer) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		h, err := renderer.Acquire(r.Context())
		if err != nil {
			slog.Error("helper: no plugin instance", "err", err)
			http.Error(w, "service unavailable", http.StatusServiceUnavailable)
			return
		}
		defer h.Release()
		writePage(w, r, helperTmpl, pageData{Framework: framework, Tasks: tasksJSON, SSR: h})
	}
}

// writePage executes the layout into a buffer first, so a template error becomes a clean 500
// instead of a half-written page.
func writePage(w http.ResponseWriter, r *http.Request, t *template.Template, data pageData) {
	var b bytes.Buffer
	if err := t.ExecuteTemplate(&b, "layout", data); err != nil {
		slog.Error("template", "path", r.URL.Path, "err", err)
		http.Error(w, "internal server error", http.StatusInternalServerError)
		return
	}
	if data.SSR != nil {
		w.Header().Set("Server-Timing", serverTiming(data.SSR.Elapsed()))
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Write(b.Bytes())
}
