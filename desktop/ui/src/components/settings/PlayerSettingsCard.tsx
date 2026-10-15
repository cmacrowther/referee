import { Lock } from 'lucide-react';
import { useEffect, useState } from 'react';

import { RadioDot, SettingsSection, ToggleRow } from '@/components/settings/SettingsPrimitives';
import type { DetectedPlayer, RendererApi, Settings } from '@/lib/types';
import { cn } from '@/lib/utils';

interface PlayerSettingsCardProps {
    api: RendererApi;
    settings: Settings;
    locked: boolean;
    onChange: (patch: Partial<Settings>) => void;
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
            role="radio"
            aria-checked={selected}
            disabled={!isSelectable}
            onClick={() => isSelectable && onSelect(player.id)}
            className={cn(
                'flex w-full items-center gap-2.5 px-3 py-1.5 text-left transition-colors',
                isSelectable ? 'hover:bg-white/[0.03]' : 'cursor-not-allowed opacity-35'
            )}
        >
            <RadioDot selected={selected} />
            <span className={cn('min-w-0 truncate text-[12px] font-medium', selected ? 'text-white' : 'text-white/80')}>
                {player.name}
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
        <SettingsSection
            title="Player"
            aside={locked && (
                <span className="flex items-center gap-1 text-[#FF6B35] opacity-90">
                    <Lock className="size-[10px]" />
                    <span className="text-[9px] font-bold uppercase tracking-widest">Locked</span>
                </span>
            )}
            groupClassName={cn(locked && 'pointer-events-none border-[#FF6B35]/20 opacity-50')}
        >
            <ToggleRow
                label="Stream Player"
                description="Open new streams automatically"
                checked={playerSettings.enabled}
                onChange={handleToggleEnabled}
            />

            {playerSettings.enabled && (
                <div className="flex flex-col py-1">
                    {detecting ? (
                        <p className="px-3 py-1.5 text-[11px] text-referee-muted">Checking available players...</p>
                    ) : (
                        <div role="radiogroup" aria-label="Stream Player" className="flex flex-col">
                            {players.map(player => (
                                <PlayerOption
                                    key={player.id}
                                    player={player}
                                    selected={playerSettings.selectedPlayer === player.id}
                                    onSelect={handleSelectPlayer}
                                />
                            ))}
                        </div>
                    )}

                    {playerSettings.selectedPlayer === 'custom' && (
                        <div className="flex flex-col gap-1 px-3 pb-1.5 pt-1">
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
                                className="w-full rounded-md border border-white/[0.08] bg-white/[0.04] px-2.5 py-1.5 text-[11px] text-white placeholder-white/25 outline-none transition-colors focus:border-referee-accent/40 focus:bg-referee-accent/[0.04]"
                            />
                        </div>
                    )}
                </div>
            )}
        </SettingsSection>
    );
}
