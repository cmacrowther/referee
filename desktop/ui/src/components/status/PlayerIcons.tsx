import { SiMpv, SiVlcmediaplayer } from '@icons-pack/react-simple-icons';
import { Play } from 'lucide-react';
import type { ReactElement } from 'react';

// VLC and MPV use official Simple Icons brand icons.
// MPC-HC and PotPlayer are not in any mainstream icon library — hand-drawn SVGs are used instead.

function MpcIcon({ className }: { className?: string }) {
    return (
        <svg className={className} viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            {/* Screen */}
            <rect x="2" y="4" width="20" height="14" rx="2" fill="#1a73e8" />
            {/* Play triangle */}
            <polygon points="9.5,8.5 16,12 9.5,15.5" fill="white" />
            {/* Stand */}
            <rect x="9" y="18" width="6" height="1.5" rx="0.75" fill="#888" />
        </svg>
    );
}

function PotPlayerIcon({ className }: { className?: string }) {
    return (
        <svg className={className} viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
            <circle cx="12" cy="12" r="10" fill="#1fa83c" />
            {/* "P" letterform */}
            <text x="7.5" y="16.5" fontSize="12" fontWeight="bold" fill="white" fontFamily="system-ui,sans-serif">P</text>
        </svg>
    );
}

const PLAYER_ICONS: Record<string, (props: { className?: string }) => ReactElement> = {
    vlc: ({ className }) => <SiVlcmediaplayer className={className} />,
    mpv: ({ className }) => <SiMpv className={className} />,
    'mpc-hc': MpcIcon,
    potplayer: PotPlayerIcon,
};

export function PlayerButtonIcon({ playerId, className }: { playerId: string | null; className?: string }) {
    if (playerId && playerId in PLAYER_ICONS) {
        const Icon = PLAYER_ICONS[playerId];
        return <Icon className={className} />;
    }
    // Fallback: generic Play icon for custom / unknown
    return <Play className={className ?? 'size-3 fill-current'} />;
}
