import React from 'react';
import { CurveCard as CurveCardType } from '../types/layout';

interface CurveCardProps {
  card: CurveCardType;
  onEdit?: (cardId: string) => void;
}

export const CurveCard: React.FC<CurveCardProps> = ({ card, onEdit }) => {
  const getTrendIcon = (trend: string) => {
    switch (trend) {
      case 'rising':
        return (
          <svg className="w-8 h-8 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6" />
          </svg>
        );
      case 'rising-from-zero':
        return (
          <svg className="w-8 h-8 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 12l3-3 3 3 4-4M8 21l4-4 4 4M3 4h18M4 4h16v12a1 1 0 01-1 1H5a1 1 0 01-1-1V4z" />
          </svg>
        );
      default:
        return (
          <svg className="w-8 h-8 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2a2 2 0 00-2 2z" />
          </svg>
        );
    }
  };

  return (
    <div className="rounded-xl border border-white/10 bg-gray-900/50 p-4">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <h3 className="font-semibold text-white">{card.title}</h3>
        <button
          onClick={() => onEdit?.(card.id)}
          className="px-3 py-1 text-xs bg-blue-600/20 text-blue-400 border border-blue-500/30 rounded-md hover:bg-blue-600/30 transition-colors"
        >
          {card.cta}
        </button>
      </div>

      {/* Temperature Source */}
      <div className="text-xs text-gray-400 mb-4">
        {card.temperatureSource}
      </div>

      {/* Graph Visualization */}
      <div className="bg-gray-800/40 rounded-lg p-3 mb-4 h-24 flex items-center justify-center">
        <div className="flex items-center gap-3">
          {getTrendIcon(card.graph.series[0]?.trend || 'rising')}
          <div className="text-xs text-gray-400">
            <div>{card.graph.xAxis}</div>
            <div className="text-gray-500">vs</div>
            <div>{card.graph.yAxis}</div>
          </div>
        </div>
      </div>

      {/* Readouts */}
      <div className="grid grid-cols-1 gap-2">
        {card.readouts.rpm && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">Current Output</div>
            <div className="text-sm font-medium text-white">{card.readouts.rpm}</div>
          </div>
        )}
        {card.readouts.percent && (
          <div className="bg-gray-800/40 rounded-lg p-2">
            <div className="text-xs text-gray-400">Current Output</div>
            <div className="text-sm font-medium text-white">{card.readouts.percent}</div>
          </div>
        )}
      </div>
    </div>
  );
};
