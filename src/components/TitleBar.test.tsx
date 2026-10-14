// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import TitleBar from "./TitleBar";

const native = vi.hoisted(() => ({
	available: true,
	maximized: false,
	resized: () => {},
	stop: vi.fn(),
	minimize: vi.fn(),
	toggleMaximize: vi.fn(),
	close: vi.fn(),
	startDragging: vi.fn(),
	isMaximized: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => native.available }));
vi.mock("@tauri-apps/api/window", () => ({
	getCurrentWindow: () => ({
		minimize: native.minimize,
		toggleMaximize: native.toggleMaximize,
		close: native.close,
		startDragging: native.startDragging,
		isMaximized: native.isMaximized,
		onResized: async (handler: () => void) => {
			native.resized = handler;
			return native.stop;
		},
	}),
}));
const props = {
	selecting: false,
	hasRecording: false,
	inspectorOpen: true,
	onOpen: vi.fn(),
	onToggleInspector: vi.fn(),
};
beforeEach(() => {
	vi.resetAllMocks();
	native.available = true;
	native.maximized = false;
	native.isMaximized.mockImplementation(async () => native.maximized);
	native.toggleMaximize.mockImplementation(async () => {
		native.maximized = !native.maximized;
	});
});
afterEach(cleanup);

it("operates native buttons and tracks maximize changes from outside React", async () => {
	const { unmount } = render(<TitleBar {...props} />);
	fireEvent.click(screen.getByRole("button", { name: "Minimize window" }));
	expect(native.minimize).toHaveBeenCalledOnce();
	fireEvent.click(screen.getByRole("button", { name: "Maximize window" }));
	await screen.findByRole("button", { name: "Restore window" });
	fireEvent.click(screen.getByRole("button", { name: "Restore window" }));
	await screen.findByRole("button", { name: "Maximize window" });
	await act(async () => {
		native.maximized = true;
		native.resized();
	});
	expect(await screen.findByRole("button", { name: "Restore window" })).toBeTruthy();
	fireEvent.click(screen.getByRole("button", { name: "Close window" }));
	expect(native.close).toHaveBeenCalledOnce();
	unmount();
	expect(native.stop).toHaveBeenCalledOnce();
});

it("limits dragging and double-click maximize to the dedicated region", async () => {
	const { container } = render(<TitleBar {...props} />);
	const drag = container.querySelector("[data-window-drag-region]")!;
	fireEvent.mouseDown(drag, { button: 0, detail: 1 });
	expect(native.startDragging).toHaveBeenCalledOnce();
	fireEvent.mouseDown(drag, { button: 0, detail: 2 });
	await screen.findByRole("button", { name: "Restore window" });
	fireEvent.mouseDown(drag, { button: 2 });
	fireEvent.mouseDown(screen.getByRole("button", { name: "Open recording" }), { button: 0 });
	fireEvent.click(screen.getByRole("button", { name: "Open recording" }));
	fireEvent.click(screen.getByRole("button", { name: "Hide details" }));
	expect(native.startDragging).toHaveBeenCalledOnce();
	expect(props.onOpen).toHaveBeenCalledOnce();
	expect(props.onToggleInspector).toHaveBeenCalledOnce();
});

it("hides operating-system controls and native drag behavior in a browser", () => {
	native.available = false;
	const { container } = render(<TitleBar {...props} />);
	expect(screen.queryByRole("group", { name: "Window controls" })).toBeNull();
	expect(container.querySelector("[data-window-drag-region]")).toBeNull();
	expect(screen.getByRole("button", { name: "Open recording" })).toBeTruthy();
});

it("reports failed commands while keeping essential controls available", async () => {
	native.close.mockRejectedValue(new Error("denied"));
	render(<TitleBar {...props} />);
	fireEvent.click(screen.getByRole("button", { name: "Close window" }));
	await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Alt+F4"));
	expect(screen.getByRole("button", { name: "Close window" }).hasAttribute("disabled")).toBe(
		false,
	);
});

it("discards a stale state response after maximizing", async () => {
	let finishInitial: (value: boolean) => void = () => {};
	native.isMaximized.mockImplementationOnce(
		() =>
			new Promise((resolve) => {
				finishInitial = resolve;
			}),
	);
	render(<TitleBar {...props} />);
	fireEvent.click(screen.getByRole("button", { name: "Maximize window" }));
	await screen.findByRole("button", { name: "Restore window" });
	await act(async () => finishInitial(false));
	expect(screen.getByRole("button", { name: "Restore window" })).toBeTruthy();
});
