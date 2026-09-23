/**
 * Public UI foundation entry point. Components are being moved here from the
 * host in small, independently verifiable steps; keeping one entry point
 * prevents new tools from importing host-internal paths.
 */
export * from "./bindings";

export { useToastStore, ToastStack } from "./components/Toast";
export type { ToastVariant } from "./components/Toast";

export { ContextMenu } from "./components/ContextMenu";
export type { ContextMenuItem } from "./components/ContextMenu";

export { ConfirmDialog } from "./components/ConfirmDialog";
export type { DialogSeverity } from "./components/ConfirmDialog";

export { ThemeToggle } from "./components/ThemeToggle";
export { useThemeStore } from "./stores/themeStore";
export type { Theme } from "./stores/themeStore";
export { useModalStackStore } from "./stores/modalStackStore";

export { formatError } from "./utils/error";
export { formatBytes } from "./utils/format";

export { useFileTreeOperations, flattenVisible, parentOf, baseName } from "./hooks/useFileTreeOperations";
export type { FileTreeBackend, UseFileTreeOperationsOptions } from "./hooks/useFileTreeOperations";

// 两份纯 CSS 文件（设计令牌 + 通用组件类），消费方在自己的入口用
// `import "@roc_desk/ui-core/styles/theme.css"` / `.../styles/core.css` 引入,
// 不通过这个 index.ts 转发（Vite/CSS 走的是各自独立的 import 图）。
