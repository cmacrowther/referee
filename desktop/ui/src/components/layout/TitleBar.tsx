import { getCurrentWindow } from '@tauri-apps/api/window';
import { ChevronLeft, Minus, Pin, SlidersHorizontal, X } from 'lucide-react';

import { cn } from '@/lib/utils';
import type { AppView } from '@/lib/types';

interface TitleBarProps {
    view: AppView;
    pinned: boolean;
    onToggleView: () => void;
    onTogglePin: () => void;
    onMinimize: () => void;
    onClose: () => void;
}

function Wordmark() {
    return (
        <span className="flex items-baseline text-[12px] font-extrabold uppercase tracking-widest text-white">
            <span>R</span>
            <span>E</span>
            <span className="text-[#FF6B35]">F</span>
            <span>E</span>
            <span>R</span>
            <span>E</span>
            <span>E</span>
        </span>
    );
}

export function TitleBar({ view, pinned, onToggleView, onTogglePin, onMinimize, onClose }: TitleBarProps) {
    return (
        <div className="z-10 flex h-[38px] min-h-[38px] items-center border-b border-[#282828] bg-referee-black">
            <div
                className="drag-region flex h-full min-w-0 flex-1 items-center pl-3"
                data-tauri-drag-region
                onMouseDown={(e) => { if (e.buttons === 1) getCurrentWindow().startDragging(); }}
            >
                <div className="flex min-w-0 items-center gap-2.5">
                    <button
                        id="nav-button"
                        className="nav-button no-drag-region"
                        type="button"
                        title={view === 'settings' ? 'Back to Status' : 'Open Settings'}
                        onClick={onToggleView}
                    >
                        {view === 'settings' ? (
                            <ChevronLeft className="size-[15px]" />
                        ) : (
                            <SlidersHorizontal className="size-[15px]" />
                        )}
                    </button>
                    <Wordmark />
                </div>
            </div>

            <div className="no-drag-region flex h-full shrink-0">
                <button
                    id="pin-button"
                    type="button"
                    aria-pressed={pinned}
                    title={pinned ? 'Disable Always on Top' : 'Keep REFEREE on top'}
                    onClick={onTogglePin}
                    className={cn('header-control-button no-drag-region', pinned && 'header-control-button-active')}
                >
                    <Pin className="size-4" />
                </button>
                <button
                    id="minimize-button"
                    type="button"
                    title="Minimize REFEREE"
                    onClick={onMinimize}
                    className="header-control-button no-drag-region"
                >
                    <Minus className="size-4" />
                </button>
                <button
                    id="close-button"
                    type="button"
                    title="Close REFEREE"
                    onClick={onClose}
                    className="no-drag-region flex h-full w-[42px] items-center justify-center text-referee-muted transition-colors duration-150 hover:bg-[#e81123] hover:text-white"
                >
                    <X className="size-4" />
                </button>
            </div>
        </div>
    );
}
