window.__ModuleLoader__.load({
  id: "kdo",
  factory: (require) => {
    const module = { exports: {} };
    const React = require("react");
    const { Button, Input } = require("@deepseek-ai/dsh-client-ui-primitives");
    const h = React.createElement;
    const CHANNEL = "/kdo";

    function unwrap(result) {
      if (result?.ok === true) return result.value;
      throw new Error(result?.error?.message ?? "Import failed");
    }

    function ImportSection({ rpc }) {
      const [source, setSource] = React.useState("claude");
      const [sourceId, setSourceId] = React.useState("");
      const [busy, setBusy] = React.useState(false);
      const [result, setResult] = React.useState();
      const [error, setError] = React.useState();
      const submit = (event) => {
        event.preventDefault();
        setBusy(true);
        setError(undefined);
        setResult(undefined);
        rpc.call(CHANNEL, "import", { source, sourceId })
          .then(unwrap)
          .then(setResult)
          .catch((cause) => setError(cause.message))
          .finally(() => setBusy(false));
      };
      return h("section", { className: "kdoImportPage" },
        h("h2", null, "kdo chats"),
        h("p", { className: "kdoImportIntro" }, "Import a local Claude Code, Codex, OpenCode, or Grok Build chat. Only visible user and assistant text is copied. Choose kdo and paste a .kdo.json path to import a file you already exported."),
        h("form", { onSubmit: submit, className: "kdoImportCard" },
          h("label", null, "Source",
            h("select", { value: source, onChange: (event) => setSource(event.currentTarget.value) },
              h("option", { value: "claude" }, "Claude Code"),
              h("option", { value: "codex" }, "Codex"),
              h("option", { value: "opencode" }, "OpenCode"),
              h("option", { value: "grok" }, "Grok Build"),
              h("option", { value: "kdo" }, ".kdo.json file"))),
          h("label", null, source === "kdo" ? "Path" : "Session id",
            h(Input, {
              value: sourceId,
              onChange: (event) => setSourceId(event.currentTarget.value),
              placeholder: source === "kdo" ? "/path/to/chat.kdo.json" : "session id",
              spellCheck: false,
              autoComplete: "off",
            })),
          h(Button, { type: "submit", variant: "primary", disabled: busy || !sourceId.trim() }, busy ? "Importing…" : "Import"),
          error ? h("p", { className: "kdoImportError", role: "alert" }, error) : null,
          result ? h("div", { className: "kdoImportSuccess", role: "status" },
            h("strong", null, "Imported"),
            h("span", null, result.title),
            h("code", null, result.sessionId)) : null));
    }

    function apply(ctx) {
      const style = document.createElement("style");
      style.dataset.plugin = "kdo";
      style.textContent = ".kdoImportPage{max-width:720px}.kdoImportIntro{color:var(--muted-foreground);line-height:1.55}.kdoImportCard{display:grid;gap:16px;padding:20px;border:1px solid var(--border);border-radius:14px;background:var(--card)}.kdoImportCard label{display:grid;gap:7px;font-weight:600}.kdoImportCard select{height:40px;border:1px solid var(--border);border-radius:8px;padding:0 10px;background:var(--background);color:inherit}.kdoImportError{color:#dc2626}.kdoImportSuccess{display:grid;gap:6px;padding:13px;border-radius:10px;background:color-mix(in srgb,#0f766e 14%,transparent)}.kdoImportSuccess code{font-size:12px}";
      document.head.append(style);
      ctx.effect(() => () => style.remove(), "kdo: style");
      const connection = ctx.get("connection");
      ctx.slots.inject("settings.section", () => ctx.slots.register({
        name: "settings.section",
        id: "kdo",
        order: 18,
        label: () => "kdo",
        inject: () => ({ rpc: connection.rpc }),
      }, ImportSection));
    }

    module.exports.apply = apply;
    module.exports.inject = ["slots", "connection"];
    return module.exports;
  },
});
