// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import Player from "./Player";

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
	Reflect.deleteProperty(document, "fullscreenEnabled");
});

it("connects transport controls to the media element and reflects media events", async () => {
	render(<Player url="asset://synthetic" name="Synthetic recording" />);
	const video = screen.getByLabelText("Synthetic recording") as HTMLVideoElement;
	Object.defineProperty(video, "duration", { configurable: true, value: 90 });
	fireEvent.loadedMetadata(video);
	expect(screen.getByLabelText("Seek").hasAttribute("disabled")).toBe(false);
	const play = vi.spyOn(video, "play").mockImplementation(async () => {
		fireEvent.play(video);
	});
	const pause = vi.spyOn(video, "pause").mockImplementation(() => {
		fireEvent.pause(video);
	});
	fireEvent.click(screen.getByRole("button", { name: "Play" }));
	await screen.findByRole("button", { name: "Pause" });
	expect(play).toHaveBeenCalledOnce();
	Object.defineProperty(video, "paused", { configurable: true, value: false });
	fireEvent.click(screen.getByRole("button", { name: "Pause" }));
	expect(pause).toHaveBeenCalledOnce();
	fireEvent.change(screen.getByLabelText("Seek"), { target: { value: "30" } });
	expect(video.currentTime).toBe(30);
	video.currentTime = 45;
	fireEvent.timeUpdate(video);
	expect((screen.getByLabelText("Seek") as HTMLInputElement).value).toBe("45");
	fireEvent.change(screen.getByLabelText("Volume"), { target: { value: "0.35" } });
	expect(video.volume).toBe(0.35);
	fireEvent.click(screen.getByRole("button", { name: "Mute" }));
	expect(video.muted).toBe(true);
	fireEvent.volumeChange(video);
	fireEvent.click(screen.getByRole("button", { name: "Unmute" }));
	expect(video.muted).toBe(false);
	fireEvent.ended(video);
	expect(screen.getByRole("button", { name: "Play" })).toBeTruthy();
});

it("reports rejected play requests without replacing the recording", async () => {
	render(<Player url="asset://synthetic" name="Synthetic recording" />);
	const video = screen.getByLabelText("Synthetic recording") as HTMLVideoElement;
	vi.spyOn(video, "play").mockRejectedValue(new Error("unsupported"));
	fireEvent.click(screen.getByRole("button", { name: "Play" }));
	expect(await screen.findByRole("alert")).toBeTruthy();
	expect(video.getAttribute("src")).toBe("asset://synthetic");
});

it("requests fullscreen for the player including transport controls", async () => {
	Object.defineProperty(document, "fullscreenEnabled", { configurable: true, value: true });
	render(<Player url="asset://synthetic" name="Synthetic recording" />);
	const player = screen.getByLabelText("Playback controls").parentElement!;
	const request = vi.fn().mockResolvedValue(undefined);
	Object.defineProperty(player, "requestFullscreen", { value: request });
	fireEvent.click(screen.getByRole("button", { name: "Toggle fullscreen" }));
	await waitFor(() => expect(request).toHaveBeenCalledOnce());
});

it.each([true, false])(
	"applies every speed preset when paused=%s and tracks external rate changes",
	(paused) => {
		render(<Player url="asset://synthetic" name="Synthetic recording" />);
		const video = screen.getByLabelText("Synthetic recording") as HTMLVideoElement;
		Object.defineProperty(video, "paused", { configurable: true, value: paused });
		const speed = screen.getByLabelText("Playback speed") as HTMLSelectElement;
		expect(speed.value).toBe("1");
		for (const rate of [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3]) {
			fireEvent.change(speed, { target: { value: String(rate) } });
			expect(video.playbackRate).toBe(rate);
			expect(video.paused).toBe(paused);
			fireEvent.rateChange(video);
			expect(speed.value).toBe(String(rate));
		}
		video.playbackRate = 1.1;
		fireEvent.rateChange(video);
		expect(speed.value).toBe("1.1");
		fireEvent.change(speed, { target: { value: "1" } });
		expect(video.playbackRate).toBe(1);
		expect(speed.value).toBe("1");
	},
);

it("keeps speed through pause, play and seek; a new recording starts at normal speed", () => {
	const { rerender } = render(<Player key="first" url="asset://first" name="First" />);
	const video = screen.getByLabelText("First") as HTMLVideoElement;
	fireEvent.change(screen.getByLabelText("Playback speed"), { target: { value: "2" } });
	fireEvent.play(video);
	fireEvent.pause(video);
	fireEvent.seeked(video);
	expect(video.playbackRate).toBe(2);
	expect((screen.getByLabelText("Playback speed") as HTMLSelectElement).value).toBe("2");
	rerender(<Player key="second" url="asset://second" name="Second" />);
	expect((screen.getByLabelText("Second") as HTMLVideoElement).playbackRate).toBe(1);
	expect((screen.getByLabelText("Playback speed") as HTMLSelectElement).value).toBe("1");
});

it("shows an actionable error and actual rate when the engine rejects a speed", () => {
	render(<Player url="asset://synthetic" name="Synthetic recording" />);
	const video = screen.getByLabelText("Synthetic recording") as HTMLVideoElement;
	Object.defineProperty(video, "playbackRate", {
		configurable: true,
		get: () => 1,
		set: () => {
			throw new Error("unsupported");
		},
	});
	fireEvent.change(screen.getByLabelText("Playback speed"), { target: { value: "3" } });
	expect(screen.getByRole("alert").textContent).toContain("does not support");
	expect((screen.getByLabelText("Playback speed") as HTMLSelectElement).value).toBe("1");
});
