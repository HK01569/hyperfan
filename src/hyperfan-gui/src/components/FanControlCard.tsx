import React, { useState } from 'react';
import { FanControlCard as FanControlCardType } from '../types/layout';

interface FanControlCardProps {
  card: FanControlCardType;
  onEdit?: (cardId: string) => void;
  onToggle?: (cardId: string, enabled: boolean) => void;
  onSliderChange?: (cardId: string, value: number) => void;
}

export const FanControlCard: React.FC<FanControlCardProps> = ({
  card,
  onEdit,
  onToggle,
  onSliderChange
}) => {
  const [showTuning, setShowTuning] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const getStatusBadgeColor = (status: string) => {
    switch (status.toLowerCase()) {
      case 'auto':
        return 'bg-green-500/20 text-green-400 border-green-500/30';
      case 'manual':
        return 'bg-blue-500/20 text-blue-400 border-blue-500/30';
      case 'calibrated':
        return 'bg-purple-500/20 text-purple-400 border-purple-500/30';
      default:
        return 'bg-gray-500/20 text-gray-400 border-gray-500/30';
    }
  };

  return (
    <div className="rounded-xl border border-white/10 bg-gray-900/50 p-4 relative">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <h3 className="font-semibold text-white">{card.title}</h3>
          <span className={`px-2 py-1 rounded-md text-xs border ${getStatusBadgeColor(card.statusBadge)}`}>
            {card.statusBadge}
          </span>
        </div>
        
        {card.actionsMenu && (
          <div className="relative">
            <button
              onClick={() => setMenuOpen(!menuOpen)}
              className="p-1 rounded-md hover:bg-gray-800/60 text-gray-400 hover:text-white"
            >
              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6v.01M12 12v.01M12 18v.01" />
              </svg>
            </button>
            
            {menuOpen && (
              <div className="absolute right-0 top-8 bg-gray-800 border border-white/10 rounded-lg shadow-lg z-10 min-w-[120px]">
                <button
                  onClick={() => {
                    onEdit?.(card.id);
                    setMenuOpen(false);
                  }}
                  className="w-full px-3 py-2 text-left text-sm text-gray-300 hover:bg-gray-700 rounded-t-lg"
                >
                  Edit Curve
                </button>
                <button
                  onClick={() => {
                    setShowTuning(!showTuning);
                    setMenuOpen(false);
                  }}
                  className="w-full px-3 py-2 text-left text-sm text-gray-300 hover:bg-gray-700"
                >
                  Tuning
                </button>
                <button
                  onClick={() => setMenuOpen(false)}
                  className="w-full px-3 py-2 text-left text-sm text-gray-300 hover:bg-gray-700 rounded-b-lg"
                >
                  Calibrate
                </button>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Curve Info */}
      <div className="text-xs text-gray-400 mb-3">
        Curve: {card.curve}
      </div>

      {/* Controls */}
      {card.controls && (
        <div className="mb-4 space-y-2">
          {card.controls.map((control, idx) => (
            <div key={idx} className="flex items-center gap-3">
              {control.type === 'slider' && (
                <div className="flex-1">
                  <input
                    type="range"
                    min="0"
                    max="100"
                    defaultValue={parseInt(control.value)}
                    onChange={(e) => onSliderChange?.(card.id, parseInt(e.target.value))}
                    className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer slider"
                  />
                  <div className="text-xs text-gray-400 mt-1">Manual: {control.value}</div>
                </div>
              )}
              {control.type === 'toggle' && (
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    defaultChecked={control.value === 'on'}
                    onChange={(e) => onToggle?.(card.id, e.target.checked)}
                    className="sr-only"
                  />
                  <div className={`w-10 h-6 rounded-full transition-colors ${
                    control.value === 'on' ? 'bg-blue-600' : 'bg-gray-600'
                  }`}>
                    <div className={`w-4 h-4 bg-white rounded-full mt-1 transition-transform ${
                      control.value === 'on' ? 'translate-x-5' : 'translate-x-1'
                    }`} />
                  </div>
                  <span className="text-xs text-gray-400">Enabled</span>
                </label>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Temperature Source and Hysteresis */}
      <div className="mb-4 space-y-3">
        {card.temperatureSource && (
          <div className="relative">
            <label className="block text-xs text-gray-400 mb-1">Temperature Source</label>
            <div className="relative">
              <select
                className="w-full bg-gray-800/40 border border-gray-700 rounded-md py-1.5 pl-3 pr-8 text-sm text-white focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                value={card.temperatureSource.selected}
                onChange={(e) => console.log('Selected source:', e.target.value)}
              >
                {card.temperatureSource.available.map((source) => (
                  <option key={source.id} value={source.id}>
                    {source.label}
                  </option>
                ))}
              </select>
              <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center px-2 text-gray-400">
                <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                </svg>
              </div>
            </div>
          </div>
        )}

        {card.hysteresis && (
          <div>
            <div className="flex justify-between text-xs text-gray-400 mb-1">
              <span>Hysteresis</span>
              <span className="text-white">±{card.hysteresis.value}%</span>
            </div>
            <input
              type="range"
              min={card.hysteresis.min}
              max={card.hysteresis.max}
              step={card.hysteresis.step}
              value={card.hysteresis.value}
              onChange={(e) => console.log('Hysteresis changed:', e.target.value)}
              className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer"
            />
          </div>
        )}
      </div>

      {/* Readouts */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        {card.readouts.percent && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">Speed</div>
            <div className="text-sm font-medium text-white">{card.readouts.percent}</div>
          </div>
        )}
        {card.readouts.rpm && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">RPM</div>
            <div className="text-sm font-medium text-white">{card.readouts.rpm}</div>
          </div>
        )}
      </div>

      {/* Tuning Panel */}
      {showTuning && card.tuning && (
        <div className="border-t border-white/10 pt-3 mt-3">
          <h4 className="text-xs font-medium text-gray-300 mb-2">Tuning Parameters</h4>
          <div className="grid grid-cols-2 gap-2 text-xs">
            <div>
              <span className="text-gray-400">Step Up:</span>
              <span className="text-white ml-1">{card.tuning.stepUp}</span>
            </div>
            <div>
              <span className="text-gray-400">Step Down:</span>
              <span className="text-white ml-1">{card.tuning.stepDown}</span>
            </div>
            <div>
              <span className="text-gray-400">Start:</span>
              <span className="text-white ml-1">{card.tuning.startPercent}</span>
            </div>
            <div>
              <span className="text-gray-400">Stop:</span>
              <span className="text-white ml-1">{card.tuning.stopPercent}</span>
            </div>
            <div>
              <span className="text-gray-400">Minimum:</span>
              <span className="text-white ml-1">{card.tuning.minimumPercent}</span>
            </div>
            <div>
              <span className="text-gray-400">Offset:</span>
              <span className="text-white ml-1">{card.tuning.offset}</span>
            </div>
          </div>
        </div>
      )}

      {/* Click outside to close menu */}
      {menuOpen && (
        <div
          className="fixed inset-0 z-0"
          onClick={() => setMenuOpen(false)}
        />
      )}
    </div>
  );
};
