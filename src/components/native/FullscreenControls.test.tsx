// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import FullscreenControls from "./FullscreenControls";

const callbacks = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/api/event", () => ({
	listen: vi.fn(async (name: string, callback: (event: { payload: unknown }) => void) => {
		callbacks.set(name, callback);
		return () => callbacks.delete(name);
	}),
}));
const snapshot = {
	id: 7,
	position: 5,
	duration: 30,
	paused: false,
	speed: 1,
	volume: 100,
	muted: false,
	error: "",
	fullscreen: true,
};

async function present(visible: boolean) {
	await act(async () => {
		callbacks.get("native-controls-state")?.({ payload: { snapshot, visible } });
	});
}

beforeEach(() => {
	vi.useFakeTimers();
	vi.stubGlobal(
		"matchMedia",
		vi.fn(() => ({ matches: false })),
	);
});
afterEach(() => {
	cleanup();
	callbacks.clear();
	vi.clearAllMocks();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

it("connects only the shared transport and never loads another video or player", async () => {
	render(<FullscreenControls />);
	await act(async () => {});
	await present(true);
	expect(invoke).toHaveBeenCalledWith("mpv_overlay", { action: { kind: "ready" } });
	expect(document.querySelector("video")).toBeNull();
	fireEvent.click(screen.getByLabelText("Pause"));
	expect(invoke).toHaveBeenCalledWith("mpv_control", {
		id: 7,
		control: { kind: "pause", value: true },
	});
	fireEvent.change(screen.getByLabelText("Playback speed"), { target: { value: "1.5" } });
	expect(invoke).toHaveBeenCalledWith("mpv_control", {
		id: 7,
		control: { kind: "speed", value: 1.5 },
	});
	await act(async () => {});
});

it("keeps the native window present for fade-out and cancels hiding on immediate reveal", async () => {
	render(<FullscreenControls />);
	await act(async () => {});
	await present(true);
	await present(false);
	expect(document.querySelector('[data-visible="false"][inert]')).toBeTruthy();
	await act(async () => {
		await vi.advanceTimersByTimeAsync(100);
	});
	expect(invoke).not.toHaveBeenCalledWith("mpv_overlay", { action: { kind: "hidden", id: 7 } });
	await present(true);
	expect(document.querySelector('[data-visible="true"]')).toBeTruthy();
	await act(async () => {
		await vi.advanceTimersByTimeAsync(200);
	});
	expect(invoke).not.toHaveBeenCalledWith("mpv_overlay", { action: { kind: "hidden", id: 7 } });
	await present(false);
	await act(async () => {
		await vi.advanceTimersByTimeAsync(180);
	});
	expect(invoke).toHaveBeenCalledWith("mpv_overlay", { action: { kind: "hidden", id: 7 } });
});

it("exits fullscreen with Escape from the owned controls window", async () => {
	render(<FullscreenControls />);
	await act(async () => {});
	await present(true);
	fireEvent.keyDown(screen.getByLabelText("Pause"), { key: "Escape" });
	expect(invoke).toHaveBeenCalledWith("mpv_control", {
		id: 7,
		control: { kind: "fullscreen", value: false },
	});
	await act(async () => {});
});

it("respects reduced motion when dismissing the controls window", async () => {
	vi.stubGlobal(
		"matchMedia",
		vi.fn(() => ({ matches: true })),
	);
	render(<FullscreenControls />);
	await act(async () => {});
	await present(false);
	await act(async () => {
		await vi.advanceTimersByTimeAsync(0);
	});
	expect(invoke).toHaveBeenCalledWith("mpv_overlay", { action: { kind: "hidden", id: 7 } });
});
