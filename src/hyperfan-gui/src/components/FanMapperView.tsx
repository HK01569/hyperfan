import React, { useEffect, useState } from 'react';
import { LayoutGrid } from './LayoutGrid';
import { loadLayoutConfig, applyTheme, validateLayout } from '../utils/layout';
import { LayoutConfig } from '../types/layout';

interface FanMapperViewProps {}

export const FanMapperView: React.FC<FanMapperViewProps> = () => {
  const [config, setConfig] = useState<LayoutConfig | null>(null);
  const [layoutErrors, setLayoutErrors] = useState<string[]>([]);

  useEffect(() => {
    // Load and validate layout configuration
    const layoutConfig = loadLayoutConfig();
    const errors = validateLayout(layoutConfig);
    
    setConfig(layoutConfig);
    setLayoutErrors(errors);
    
    // Apply theme
    applyTheme(layoutConfig.app.theme);
  }, []);

  const handleCardEdit = (cardId: string) => {
    console.log('Edit card:', cardId);
    // TODO: Implement curve editor integration
    // This should check profile validation as per memory requirements
  };

  const handleFanToggle = (cardId: string, enabled: boolean) => {
    console.log('Toggle fan:', cardId, enabled);
    // TODO: Implement PWM control
  };

  const handleSliderChange = (cardId: string, value: number) => {
    console.log('Slider change:', cardId, value);
    // TODO: Implement manual PWM control
  };

  const handleFunctionChange = (cardId: string, func: string) => {
    console.log('Function change:', cardId, func);
    // TODO: Implement mixer function change
  };

  const handleCurveToggle = (cardId: string, curve: string, active: boolean) => {
    console.log('Curve toggle:', cardId, curve, active);
    // TODO: Implement curve mixing
  };

  if (!config) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-gray-400">Loading layout configuration...</div>
      </div>
    );
  }

  if (layoutErrors.length > 0) {
    return (
      <div className="bg-red-900/20 border border-red-500/30 rounded-lg p-4">
        <h3 className="text-red-400 font-semibold mb-2">Layout Configuration Errors</h3>
        <ul className="text-red-300 text-sm space-y-1">
          {layoutErrors.map((error, idx) => (
            <li key={idx}>• {error}</li>
          ))}
        </ul>
      </div>
    );
  }

  return (
    <div className="min-h-screen" style={{ backgroundColor: config.app.theme.background }}>
      {/* Header */}
      <header className="border-b border-white/10" style={{ height: `${config.header.heightPx}px` }}>
        <div className="flex items-center justify-between h-full px-4">
          <div className="flex items-center gap-3">
            {config.header.left.map((item, idx) => (
              <div key={idx} className="flex items-center">
                {item.type === 'title' && (
                  <h1 className="text-xl font-bold" style={{ color: config.app.theme.text }}>
                    {item.text}
                  </h1>
                )}
                {item.type === 'app-icon' && (
                  <div className="w-8 h-8 rounded-lg bg-blue-600/20 flex items-center justify-center">
                    <svg className="w-5 h-5 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 3v3m0 12v3m8-8h-3M7 12H4m11.95-4.95l-2.12 2.12M8.17 15.83l-2.12 2.12M15.83 15.83l2.12 2.12M6.05 6.05l2.12 2.12" />
                    </svg>
                  </div>
                )}
              </div>
            ))}
          </div>
          
          <div className="flex items-center gap-2">
            {config.header.right.map((item, idx) => (
              <button
                key={idx}
                className="p-2 rounded-md hover:bg-white/10 text-gray-400 hover:text-white"
                title={item.tooltip}
              >
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  {item.icon === 'visibility' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                  )}
                  {item.icon === 'more_vert' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6v.01M12 12v.01M12 18v.01" />
                  )}
                </svg>
              </button>
            ))}
          </div>
        </div>
      </header>

      <div className="flex">
        {/* Sidebar */}
        <aside 
          className="border-r border-white/10 flex flex-col"
          style={{ width: `${config.sidebar.widthPx}px` }}
        >
          <nav className="flex-1 p-2">
            {config.sidebar.items.map((item) => (
              <button
                key={item.id}
                className={`w-full flex flex-col items-center gap-1 p-3 rounded-lg mb-2 transition-colors ${
                  item.active 
                    ? 'bg-blue-600/20 text-blue-400' 
                    : 'text-gray-400 hover:text-white hover:bg-white/5'
                }`}
              >
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  {item.icon === 'home' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
                  )}
                  {item.icon === 'trending_up' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6" />
                  )}
                  {item.icon === 'thermostat' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2a2 2 0 00-2 2z" />
                  )}
                  {item.icon === 'bookmark' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 5a2 2 0 012-2h10a2 2 0 012 2v16l-7-3.5L5 21V5z" />
                  )}
                  {item.icon === 'settings' && (
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  )}
                </svg>
                <span className="text-xs">{item.label}</span>
              </button>
            ))}
          </nav>
        </aside>

        {/* Main Content */}
        <main className="flex-1">
          <LayoutGrid
            config={config}
            onCardEdit={handleCardEdit}
            onFanToggle={handleFanToggle}
            onSliderChange={handleSliderChange}
            onFunctionChange={handleFunctionChange}
            onCurveToggle={handleCurveToggle}
          />
        </main>
      </div>
    </div>
  );
};
