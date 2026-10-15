// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import useVideoFullscreen from "./useVideoFullscreen";

afterEach(() => {
	cleanup();
	vi.useRealTimers();
});

function Harness({
	fullscreen,
	x = 10,
	exit,
	onChange,
}: {
	fullscreen: boolean;
	x?: number;
	exit: () => void;
	onChange?: (value: boolean) => void;
}) {
	const view = useVideoFullscreen({ fullscreen, pointer: { x, y: 10 }, exit, onChange });
	return (
		<div onPointerMove={view.onPointerMove}>
			<div
				ref={view.transport}
				hidden={!view.controlsVisible}
				role="group"
				aria-label="Transport"
			>
				<button>Play</button>
				<button ref={view.fullscreenButton}>Fullscreen</button>
			</div>
		</div>
	);
}

it("auto-hides controls, then restores them on native pointer movement", () => {
	vi.useFakeTimers();
	const exit = vi.fn();
	const view = render(<Harness fullscreen exit={exit} />);
	act(() => {
		vi.advanceTimersByTime(499);
	});
	expect(screen.getByRole("group").hidden).toBe(false);
	act(() => {
		vi.advanceTimersByTime(1);
	});
	expect(screen.getByRole("group", { hidden: true }).hidden).toBe(true);
	view.rerender(<Harness fullscreen x={40} exit={exit} />);
	expect(screen.getByRole("group").hidden).toBe(false);
});

it("reveals hidden controls for Tab and keeps them available for keyboard use", () => {
	vi.useFakeTimers();
	render(<Harness fullscreen exit={vi.fn()} />);
	act(() => {
		vi.advanceTimersByTime(2600);
	});
	fireEvent.keyDown(window, { key: "Tab" });
	act(() => {
		vi.advanceTimersByTime(4000);
	});
	expect(screen.getByRole("group").hidden).toBe(false);
	expect(document.activeElement?.textContent).toBe("Play");
});

it("handles Escape while controls are hidden and restores focus and shell state on exit", () => {
	vi.useFakeTimers();
	const exit = vi.fn();
	const onChange = vi.fn();
	const view = render(<Harness fullscreen exit={exit} onChange={onChange} />);
	act(() => {
		vi.advanceTimersByTime(2600);
	});
	fireEvent.keyDown(window, { key: "Escape" });
	expect(exit).toHaveBeenCalledOnce();
	view.rerender(<Harness fullscreen={false} exit={exit} onChange={onChange} />);
	expect(onChange).toHaveBeenLastCalledWith(false);
	expect(screen.getByRole("group").hidden).toBe(false);
	expect(document.activeElement?.textContent).toBe("Fullscreen");
});

it("cleans up fullscreen presentation and idle timers on unmount", () => {
	vi.useFakeTimers();
	const onChange = vi.fn();
	const { unmount } = render(<Harness fullscreen exit={vi.fn()} onChange={onChange} />);
	unmount();
	expect(onChange).toHaveBeenLastCalledWith(false);
	expect(vi.getTimerCount()).toBe(0);
});
