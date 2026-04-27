import { Lock } from 'lucide-react';
import { useEffect, useState } from 'react';

import type { DetectedPlayer, RendererApi, Settings } from '@/lib/types';

interface PlayerSettingsCardProps {
    api: RendererApi;
    settings: Settings;
    locked: boolean;
    onChange: (patch: Partial<Settings>) => void;
}

function ToggleField({
    checked,
    onChange
}: {
    checked: boolean;
    onChange: (checked: boolean) => void;
}) {
    return (
        <label className="relative inline-flex cursor-pointer items-center">
            <input
                type="checkbox"
                className="sr-only toggle-checkbox"
                checked={checked}
                onChange={event => {
                    onChange(event.currentTarget.checked);
                }}
            />
            <div className="toggle-label h-[20px] w-[36px] rounded-full bg-[#535353] after:absolute after:left-[2px] after:top-[2px] after:h-[16px] after:w-[16px] after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all after:content-[''] hover:bg-[#666666]" />
        </label>
    );
}

function PlayerOption({
    player,
    selected,
    onSelect
}: {
    player: DetectedPlayer;
    selected: boolean;
    onSelect: (id: string) => void;
}) {
    const isCustom = player.id === 'custom';
    const isSelectable = player.installed || isCustom;

    return (
        <button
            type="button"
            disabled={!isSelectable}
            onClick={() => isSelectable && onSelect(player.id)}
            className={[
                'flex w-full items-center gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors',
                selected
                    ? 'border-[rgba(255,107,53,0.4)] bg-[rgba(255,107,53,0.08)]'
                    : isSelectable
                        ? 'border-[rgba(255,255,255,0.06)] bg-[rgba(255,255,255,0.03)] hover:border-[rgba(255,255,255,0.1)] hover:bg-[rgba(255,255,255,0.05)]'
                        : 'cursor-not-allowed border-[rgba(255,255,255,0.03)] bg-transparent opacity-35',
            ].join(' ')}
        >
            {/* Radio dot */}
            <span
                className={[
                    'mt-px flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center rounded-full border',
                    selected
                        ? 'border-[rgba(255,107,53,0.8)] bg-[rgba(255,107,53,0.8)]'
                        : 'border-[rgba(255,255,255,0.25)]',
                ].join(' ')}
                aria-hidden="true"
            >
                {selected && (
                    <span className="h-1.5 w-1.5 rounded-full bg-white" />
                )}
            </span>

            <span className="min-w-0 flex-1">
                <span className="block text-[12px] font-medium leading-tight text-white">
                    {player.name}
                </span>
            </span>
        </button>
    );
}

export function PlayerSettingsCard({ api, settings, locked, onChange }: PlayerSettingsCardProps) {
    const [players, setPlayers] = useState<DetectedPlayer[]>([]);
    const [detecting, setDetecting] = useState(true);

    const playerSettings = settings.player;

    useEffect(() => {
        api.detectPlayers()
            .then(detected => setPlayers(detected))
            .catch(() => setPlayers([]))
            .finally(() => setDetecting(false));
    }, [api]);

    function handleToggleEnabled(enabled: boolean) {
        onChange({ player: { ...playerSettings, enabled } });
    }

    function handleSelectPlayer(id: string) {
        onChange({ player: { ...playerSettings, selectedPlayer: id } });
    }

    function handleCustomPathChange(value: string) {
        onChange({ player: { ...playerSettings, customPath: value } });
    }

    return (
        <div
            className={`rounded-lg border bg-referee-card p-3.5 transition-colors duration-300 ${
                locked ? 'border-[#FF6B35]/20' : 'border-referee-border'
            }`}
        >
            <div className="mb-3 flex items-center justify-between">
                <div className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">Player</div>
                {locked && (
                    <div className="flex items-center gap-1 text-[#FF6B35] opacity-90">
                        <Lock className="size-[10px] pb-[1px]" />
                        <span className="text-[9px] font-bold uppercase tracking-widest">Locked</span>
                    </div>
                )}
            </div>

            <div className={`flex flex-col gap-4${locked ? ' pointer-events-none opacity-50' : ''}`}>
                <div className="flex items-center justify-between gap-3">
                    <div className="flex min-w-0 flex-col pr-2">
                        <span className="text-[13px] font-medium text-white">Stream Player</span>
                        <span className="text-[11px] text-referee-muted">
                            Automatically open streams in the selected player
                        </span>
                    </div>
                    <ToggleField checked={playerSettings.enabled} onChange={handleToggleEnabled} />
                </div>

                {playerSettings.enabled && (
                    <div className="flex flex-col gap-2">
                        {detecting ? (
                            <p className="text-[11px] text-referee-muted">Checking available players...</p>
                        ) : (
                            players.map(player => (
                                <PlayerOption
                                    key={player.id}
                                    player={player}
                                    selected={playerSettings.selectedPlayer === player.id}
                                    onSelect={handleSelectPlayer}
                                />
                            ))
                        )}

                        {playerSettings.selectedPlayer === 'custom' && (
                            <div className="mt-1 flex flex-col gap-1">
                                <label
                                    htmlFor="custom-player-path"
                                    className="text-[10px] font-medium uppercase tracking-wider text-referee-muted"
                                >
                                    Executable Path
                                </label>
                                <input
                                    id="custom-player-path"
                                    type="text"
                                    spellCheck={false}
                                    value={playerSettings.customPath ?? ''}
                                    onChange={e => handleCustomPathChange(e.currentTarget.value)}
                                    placeholder={
                                        typeof window !== 'undefined' && navigator.userAgent.includes('Windows')
                                            ? 'C:\\Program Files\\MyPlayer\\player.exe'
                                            : '/usr/bin/myplayer'
                                    }
                                    className="w-full rounded-lg border border-[rgba(255,255,255,0.08)] bg-[rgba(255,255,255,0.04)] px-3 py-2 text-[11px] text-white placeholder-[rgba(255,255,255,0.25)] outline-none transition-colors focus:border-[rgba(255,107,53,0.4)] focus:bg-[rgba(255,107,53,0.04)]"
                                />
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
}
