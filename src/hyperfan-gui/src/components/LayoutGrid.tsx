import React from 'react';
import { LayoutConfig, Card } from '../types/layout';
import { FanControlCard } from './FanControlCard';
import { CurveCard } from './CurveCard';
import { MixerCard } from './MixerCard';
import { SensorCard } from './SensorCard';

interface LayoutGridProps {
  config: LayoutConfig;
  onCardEdit?: (cardId: string) => void;
  onFanToggle?: (cardId: string, enabled: boolean) => void;
  onSliderChange?: (cardId: string, value: number) => void;
  onFunctionChange?: (cardId: string, func: string) => void;
  onCurveToggle?: (cardId: string, curve: string, active: boolean) => void;
}

export const LayoutGrid: React.FC<LayoutGridProps> = ({
  config,
  onCardEdit,
  onFanToggle,
  onSliderChange,
  onFunctionChange,
  onCurveToggle
}) => {
  const renderCard = (card: Card) => {
    const gridStyle = {
      gridColumn: `${card.position.col} / span ${card.position.w}`,
      gridRow: `${card.position.row} / span ${card.position.h}`,
    };

    switch (card.type) {
      case 'fan-control':
        return (
          <div key={card.id} style={gridStyle}>
            <FanControlCard
              card={card}
              onEdit={onCardEdit}
              onToggle={onFanToggle}
              onSliderChange={onSliderChange}
            />
          </div>
        );
      case 'curve-card':
        return (
          <div key={card.id} style={gridStyle}>
            <CurveCard card={card} onEdit={onCardEdit} />
          </div>
        );
      case 'mixer-card':
        return (
          <div key={card.id} style={gridStyle}>
            <MixerCard
              card={card}
              onFunctionChange={onFunctionChange}
              onCurveToggle={onCurveToggle}
            />
          </div>
        );
      case 'sensor-card':
        return (
          <div key={card.id} style={gridStyle}>
            <SensorCard card={card} />
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div 
      className="w-full"
      style={{ padding: `${config.main.containerPaddingPx}px` }}
    >
      {config.main.sections.map((section) => (
        <div key={section.id} className="mb-8">
          {/* Section Header */}
          <div className="flex items-center gap-3 mb-4">
            <h2 className="text-xl font-semibold text-white">{section.title}</h2>
            {section.badges?.map((badge, idx) => (
              <span
                key={idx}
                className="px-2 py-1 bg-blue-500/20 text-blue-400 border border-blue-500/30 rounded-md text-xs"
              >
                {badge.text}
              </span>
            ))}
          </div>

          {/* Grid Container */}
          <div
            className="grid gap-4"
            style={{
              gridTemplateColumns: `repeat(${config.main.grid.columns}, 1fr)`,
              gap: `${config.main.grid.gutterPx}px`,
              gridAutoRows: `${config.main.grid.rowHeightPx}px`,
            }}
          >
            {section.cards.map(renderCard)}
          </div>
        </div>
      ))}
    </div>
  );
};
