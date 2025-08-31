import React from 'react';
import { SensorCard as SensorCardType } from '../types/layout';

interface SensorCardProps {
  card: SensorCardType;
}

export const SensorCard: React.FC<SensorCardProps> = ({ card }) => {
  const getStatusColor = (status: string) => {
    switch (status.toLowerCase()) {
      case 'normal':
        return 'bg-green-500/20 text-green-400 border-green-500/30';
      case 'warning':
        return 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30';
      case 'critical':
        return 'bg-red-500/20 text-red-400 border-red-500/30';
      default:
        return 'bg-gray-500/20 text-gray-400 border-gray-500/30';
    }
  };

  const getTemperatureColor = (temp: string) => {
    const value = parseFloat(temp);
    if (value < 40) return 'text-blue-400';
    if (value < 60) return 'text-green-400';
    if (value < 80) return 'text-yellow-400';
    return 'text-red-400';
  };

  return (
    <div className="rounded-xl border border-white/10 bg-gray-900/50 p-4">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <h3 className="font-semibold text-white">{card.title}</h3>
        <span className={`px-2 py-1 rounded-md text-xs border ${getStatusColor(card.status)}`}>
          {card.status}
        </span>
      </div>

      {/* Current Temperature */}
      <div className="text-center mb-4">
        <div className={`text-2xl font-bold ${getTemperatureColor(card.currentTemp)}`}>
          {card.currentTemp}
        </div>
        <div className="text-xs text-gray-400">Current</div>
      </div>

      {/* Temperature Range */}
      <div className="grid grid-cols-2 gap-2">
        <div className="bg-gray-800/40 rounded-lg p-2 text-center">
          <div className="text-xs text-gray-400">Min</div>
          <div className="text-sm font-medium text-blue-400">{card.minTemp}</div>
        </div>
        <div className="bg-gray-800/40 rounded-lg p-2 text-center">
          <div className="text-xs text-gray-400">Max</div>
          <div className="text-sm font-medium text-red-400">{card.maxTemp}</div>
        </div>
      </div>

      {/* Temperature Bar Visualization */}
      <div className="mt-3">
        <div className="w-full bg-gray-700 rounded-full h-2">
          <div 
            className="bg-gradient-to-r from-blue-500 via-green-500 via-yellow-500 to-red-500 h-2 rounded-full"
            style={{ 
              width: `${Math.min(100, Math.max(0, (parseFloat(card.currentTemp) / 100) * 100))}%` 
            }}
          />
        </div>
        <div className="flex justify-between text-xs text-gray-500 mt-1">
          <span>0°C</span>
          <span>100°C</span>
        </div>
      </div>
    </div>
  );
};
