// @vitest-environment jsdom
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import TrimTimeline from "./TrimTimeline";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
	cleanup();
	vi.resetAllMocks();
	vi.unstubAllGlobals();
});

it("clears the old storyboard immediately when the editor session changes", async () => {
	vi.stubGlobal("URL", {
		createObjectURL: vi.fn(() => "blob:old-frame"),
		revokeObjectURL: vi.fn(),
	});
	vi.mocked(invoke).mockResolvedValue(new Uint8Array([255, 216, 255, 217]).buffer);
	const props = {
		token: 1,
		duration: 10,
		start: 0,
		end: 10,
		position: 0,
		disabled: false,
		onRange: vi.fn(),
		onCommit: vi.fn(),
		onSeek: vi.fn(),
	};
	const view = render(<TrimTimeline {...props} />);
	await act(async () => {});
	expect(view.container.querySelectorAll("img")).toHaveLength(8);
	vi.mocked(invoke).mockReturnValue(new Promise(() => {}));
	view.rerender(<TrimTimeline {...props} token={2} />);
	expect(view.container.querySelectorAll("img")).toHaveLength(0);
	expect(URL.revokeObjectURL).toHaveBeenCalledTimes(8);
});
