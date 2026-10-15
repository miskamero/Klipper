// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { StrictMode } from "react";
import NativePlayer from "./NativePlayer";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
const initial = {
	id: 1,
	position: 5,
	duration: 30,
	paused: false,
	speed: 1,
	volume: 100,
	hwdec: "d3d11va",
	dropped: 0,
	error: "",
	fullscreen: false,
};
afterEach(async () => {
	cleanup();
	await Promise.resolve();
	vi.useRealTimers();
	vi.resetAllMocks();
});
it("loads only an authorized ID and connects React controls to typed native requests", async () => {
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "mpv_surface") return undefined;
		if ((args as { control: { kind: string } }).control.kind === "stop") return undefined;
		const control = (args as { control: { kind: string; value?: number } }).control;
		return { ...initial, speed: control.kind === "speed" ? control.value : 1 };
	});
	render(<NativePlayer id={1} selecting={false} />);
	await screen.findByText(/d3d11va/);
	expect(invoke).toHaveBeenCalledWith("mpv_control", { id: 1, control: { kind: "load" } });
	fireEvent.change(screen.getByLabelText("Playback speed"), { target: { value: "1.5" } });
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("mpv_control", {
			id: 1,
			control: { kind: "speed", value: 1.5 },
		}),
	);
	fireEvent.change(screen.getByLabelText("Volume"), { target: { value: "30" } });
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("mpv_control", {
			id: 1,
			control: { kind: "volume", value: 30 },
		}),
	);
	fireEvent.click(screen.getByLabelText("Pause"));
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("mpv_control", {
			id: 1,
			control: { kind: "pause", value: true },
		}),
	);
});
it("ignores a late response from an unmounted recording", async () => {
	let finish!: (value: unknown) => void;
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "mpv_surface") return undefined;
		if ((args as { control: { kind: string } }).control.kind === "stop") return undefined;
		if ((args as { id: number }).id === 1)
			return new Promise((resolve) => {
				finish = resolve;
			});
		return { ...initial, id: 2, hwdec: "second recording" };
	});
	const view = render(<NativePlayer key={1} id={1} selecting={false} />);
	view.rerender(<NativePlayer key={2} id={2} selecting={false} />);
	await screen.findByText(/second recording/);
	finish({ ...initial, error: "stale error" });
	await waitFor(() => expect(screen.queryByText(/stale error/)).toBeNull());
});

it("keeps load failures visible and retries without falling back to HTML", async () => {
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "mpv_surface") return undefined;
		const control = (args as { control: { kind: string } }).control;
		if (control.kind === "load") throw new Error("libmpv DLL unavailable");
		return initial;
	});
	render(<NativePlayer id={1} selecting={false} />);
	await screen.findByText(/libmpv DLL unavailable/);
	expect(document.querySelector("video")).toBeNull();
	expect(screen.getByLabelText("Seek").hasAttribute("disabled")).toBe(true);
	fireEvent.click(screen.getByRole("button", { name: "Retry playback" }));
	await screen.findByText(/d3d11va/);
	expect(invoke).toHaveBeenCalledWith("mpv_control", { id: 1, control: { kind: "reload" } });
});

it("stops and hides the native recording on unmount", async () => {
	vi.mocked(invoke).mockResolvedValue(initial);
	const { unmount } = render(<NativePlayer id={1} selecting={false} />);
	await screen.findByText(/d3d11va/);
	unmount();
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("mpv_control", { id: 1, control: { kind: "stop" } }),
	);
	expect(invoke).toHaveBeenCalledWith("mpv_surface", {
		id: 1,
		bounds: { x: 0, y: 0, width: 0, height: 0, visible: false },
	});
});

it("keeps the replacement surface visible during development StrictMode effect replay", async () => {
	vi.mocked(invoke).mockResolvedValue(initial);
	const { unmount } = render(
		<StrictMode>
			<NativePlayer id={1} selecting={false} />
		</StrictMode>,
	);
	await screen.findByText(/d3d11va/);
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith(
			"mpv_surface",
			expect.objectContaining({
				id: 1,
				bounds: expect.objectContaining({ visible: true }),
			}),
		),
	);
	expect(invoke).not.toHaveBeenCalledWith(
		"mpv_surface",
		expect.objectContaining({
			bounds: expect.objectContaining({ visible: false }),
		}),
	);
	expect(invoke).not.toHaveBeenCalledWith("mpv_control", { id: 1, control: { kind: "stop" } });
	unmount();
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("mpv_control", { id: 1, control: { kind: "stop" } }),
	);
});

it("enters video presentation and exits with Escape without reloading playback", async () => {
	let fullscreen = false;
	const onFullscreenChange = vi.fn();
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "mpv_surface") return undefined;
		const control = (args as { control: { kind: string; value?: boolean } }).control;
		if (control.kind === "fullscreen") fullscreen = !!control.value;
		return { ...initial, fullscreen };
	});
	render(<NativePlayer id={1} selecting={false} onFullscreenChange={onFullscreenChange} />);
	await screen.findByText(/d3d11va/);
	fireEvent.click(screen.getByLabelText("Toggle fullscreen"));
	await waitFor(() => expect(onFullscreenChange).toHaveBeenLastCalledWith(true));
	expect(document.querySelector('[data-video-fullscreen="true"]')).toBeTruthy();
	fireEvent.keyDown(window, { key: "Escape" });
	await waitFor(() => expect(onFullscreenChange).toHaveBeenLastCalledWith(false));
	expect(invoke).toHaveBeenCalledWith("mpv_control", {
		id: 1,
		control: { kind: "fullscreen", value: false },
	});
	expect(
		vi
			.mocked(invoke)
			.mock.calls.filter(
				([name, args]) =>
					name === "mpv_control" &&
					(args as { control: { kind: string } }).control.kind === "load",
			),
	).toHaveLength(1);
});

it("reports window transition failures without treating them as decoding failures", async () => {
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "mpv_surface") return undefined;
		if ((args as { control: { kind: string } }).control.kind === "fullscreen")
			throw new Error("Window unavailable");
		return initial;
	});
	render(<NativePlayer id={1} selecting={false} />);
	await screen.findByText(/d3d11va/);
	fireEvent.click(screen.getByLabelText("Toggle fullscreen"));
	await screen.findByText(/Fullscreen could not change/);
	expect(screen.getByLabelText("Seek").hasAttribute("disabled")).toBe(false);
});

it("hides the cursor with idle fullscreen controls and restores it outside the player", async () => {
	vi.useFakeTimers();
	let pointer: { x: number; y: number } | null = { x: 10, y: 10 };
	vi.mocked(invoke).mockImplementation(async (name) => {
		if (name === "mpv_surface") return undefined;
		return { ...initial, fullscreen: true, pointer };
	});
	render(<NativePlayer id={1} selecting={false} />);
	await act(async () => {
		await Promise.resolve();
	});
	await act(async () => {
		await vi.advanceTimersByTimeAsync(600);
	});
	await act(async () => {
		await vi.advanceTimersByTimeAsync(32);
	});
	expect(document.querySelector('[data-cursor-hidden="true"]')).toBeTruthy();
	const transport = screen.getByRole("group", { hidden: true });
	// The fullscreen transport lives in the owned controls window, never below the video.
	expect(transport.hidden).toBe(true);
	expect(invoke).toHaveBeenCalledWith(
		"mpv_surface",
		expect.objectContaining({
			bounds: expect.objectContaining({ cursorHidden: true }),
		}),
	);
	pointer = null;
	await act(async () => {
		await vi.advanceTimersByTimeAsync(150);
	});
	await act(async () => {
		await vi.advanceTimersByTimeAsync(32);
	});
	expect(document.querySelector('[data-cursor-hidden="false"]')).toBeTruthy();
	expect(
		vi
			.mocked(invoke)
			.mock.calls.filter(([name]) => name === "mpv_surface")
			.at(-1)?.[1],
	).toEqual(
		expect.objectContaining({
			bounds: expect.objectContaining({ cursorHidden: false }),
		}),
	);
});
