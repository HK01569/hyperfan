import React, { useState } from 'react';
import { MixerCard as MixerCardType } from '../types/layout';

interface MixerCardProps {
  card: MixerCardType;
  onFunctionChange?: (cardId: string, func: string) => void;
  onCurveToggle?: (cardId: string, curve: string, active: boolean) => void;
}

export const MixerCard: React.FC<MixerCardProps> = ({ 
  card, 
  onFunctionChange, 
  onCurveToggle 
}) => {
  const [showMixing, setShowMixing] = useState(false);

  const getFunctionColor = (func: string) => {
    switch (func.toLowerCase()) {
      case 'max':
        return 'bg-red-500/20 text-red-400 border-red-500/30';
      case 'min':
        return 'bg-blue-500/20 text-blue-400 border-blue-500/30';
      case 'average':
        return 'bg-green-500/20 text-green-400 border-green-500/30';
      default:
        return 'bg-gray-500/20 text-gray-400 border-gray-500/30';
    }
  };

  return (
    <div className="rounded-xl border border-white/10 bg-gray-900/50 p-4">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <h3 className="font-semibold text-white">{card.title}</h3>
          <span className={`px-2 py-1 rounded-md text-xs border ${getFunctionColor(card.function)}`}>
            {card.function}
          </span>
        </div>
        
        <button
          onClick={() => setShowMixing(!showMixing)}
          className="p-1 rounded-md hover:bg-gray-800/60 text-gray-400 hover:text-white"
        >
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 100 4m0-4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 100 4m0-4v2m0-6V4" />
          </svg>
        </button>
      </div>

      {/* Function Selector */}
      <div className="mb-4">
        <div className="text-xs text-gray-400 mb-2">Mixing Function</div>
        <select
          value={card.function}
          onChange={(e) => onFunctionChange?.(card.id, e.target.value)}
          className="w-full bg-gray-800/70 border border-white/10 rounded-md px-3 py-2 text-sm text-white focus:outline-none focus:ring-1 focus:ring-blue-500"
        >
          <option value="Max">Maximum</option>
          <option value="Min">Minimum</option>
          <option value="Average">Average</option>
        </select>
      </div>

      {/* Active Curves Summary */}
      <div className="mb-4">
        <div className="text-xs text-gray-400 mb-2">Active Curves ({card.mixing.active.length})</div>
        <div className="space-y-1">
          {card.mixing.active.map((curve, idx) => (
            <div key={idx} className="text-xs bg-gray-800/40 rounded px-2 py-1 text-gray-300">
              {curve}
            </div>
          ))}
        </div>
      </div>

      {/* Mixing Panel */}
      {showMixing && (
        <div className="border-t border-white/10 pt-3 mb-4">
          <h4 className="text-xs font-medium text-gray-300 mb-2">Available Curves</h4>
          <div className="space-y-2">
            {card.mixing.availableFanCurves.map((curve, idx) => (
              <label key={idx} className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={card.mixing.active.includes(curve)}
                  onChange={(e) => onCurveToggle?.(card.id, curve, e.target.checked)}
                  className="w-3 h-3 text-blue-600 bg-gray-800 border-gray-600 rounded focus:ring-blue-500"
                />
                <span className="text-xs text-gray-300">{curve}</span>
              </label>
            ))}
          </div>
        </div>
      )}

      {/* Readouts */}
      <div className="grid grid-cols-1 gap-2">
        {card.readouts.rpm && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">Mixed Output</div>
            <div className="text-sm font-medium text-white">{card.readouts.rpm}</div>
          </div>
        )}
        {card.readouts.percent && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">Mixed Output</div>
            <div className="text-sm font-medium text-white">{card.readouts.percent}</div>
          </div>
        )}
      </div>
    </div>
  );
};
