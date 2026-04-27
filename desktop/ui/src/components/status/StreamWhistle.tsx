import { Cloud, Loader2 } from 'lucide-react';
import { memo, useLayoutEffect, useRef } from 'react';

import { ACTIVE_STREAM_MARKUP, decorateStreamMarkup } from '@/lib/active-stream-markup';
import { cn } from '@/lib/utils';

export type StreamWhistleState = 'inactive' | 'active';
export type StreamWhistleBadgeState = 'hidden' | 'starting' | 'live';

interface StreamWhistleProps {
    state?: StreamWhistleState;
    badgeState?: StreamWhistleBadgeState;
    className?: string;
}

export const StreamWhistle = memo(function StreamWhistle({ state = 'inactive', badgeState = 'hidden', className }: StreamWhistleProps) {
    const inactiveContainerRef = useRef<HTMLDivElement | null>(null);
    const activeContainerRef = useRef<HTMLDivElement | null>(null);

    useLayoutEffect(() => {
        decorateStreamMarkup(inactiveContainerRef.current);
        decorateStreamMarkup(activeContainerRef.current);
    }, []);

    return (
        <div className={cn('stream-whistle', className)} data-whistle-state={state}>
            <div className="stream-whistle-stage">
                <div className="stream-whistle-layer stream-whistle-layer-inactive" aria-hidden={state === 'active'}>
                    <div className="stream-whistle-inactive">
                        <div
                            ref={inactiveContainerRef}
                            className="stream-whistle-inactive-mark"
                            dangerouslySetInnerHTML={{ __html: ACTIVE_STREAM_MARKUP }}
                        />
                    </div>
                </div>

                <div className="stream-whistle-layer stream-whistle-layer-active" aria-hidden={state === 'inactive'}>
                    <div className="stream-whistle-active">
                        <div className="stream-whistle-active-ring stream-whistle-active-ring-1" aria-hidden="true" />
                        <div className="stream-whistle-active-ring stream-whistle-active-ring-2" aria-hidden="true" />
                        <div className="stream-whistle-active-ring stream-whistle-active-ring-3" aria-hidden="true" />
                        <div className="stream-whistle-active-shadow" aria-hidden="true" />
                        <div
                            ref={activeContainerRef}
                            className="stream-whistle-active-mark"
                            dangerouslySetInnerHTML={{ __html: ACTIVE_STREAM_MARKUP }}
                        />
                    </div>
                </div>
            </div>

            <div className="stream-whistle-badge" data-badge-state={badgeState} aria-hidden="true">
                <div className="stream-whistle-badge-content">
                    <span className="stream-whistle-badge-icon-slot stream-whistle-badge-icon-slot-loading">
                        <Loader2 className="stream-whistle-badge-icon stream-whistle-badge-icon-spinning" />
                    </span>
                    <span className="stream-whistle-badge-icon-slot stream-whistle-badge-icon-slot-live">
                        <Cloud className="stream-whistle-badge-icon" />
                    </span>
                </div>
            </div>
        </div>
    );
});
