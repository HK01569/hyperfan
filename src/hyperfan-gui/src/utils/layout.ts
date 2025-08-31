import { LayoutConfig } from '../types/layout';
import layoutConfigJson from '../layout-config.json';

/**
 * Load the layout configuration from JSON
 */
export const loadLayoutConfig = (): LayoutConfig => {
  return layoutConfigJson as LayoutConfig;
};

/**
 * Apply theme colors to CSS custom properties
 */
export const applyTheme = (theme: LayoutConfig['app']['theme']) => {
  const root = document.documentElement;
  root.style.setProperty('--primary-color', theme.primaryColor);
  root.style.setProperty('--background-color', theme.background);
  root.style.setProperty('--card-color', theme.card);
  root.style.setProperty('--text-color', theme.text);
};

/**
 * Calculate grid position styles for a card
 */
export const getGridStyles = (
  position: { row: number; col: number; w: number; h: number }
) => {
  return {
    gridColumn: `${position.col} / span ${position.w}`,
    gridRow: `${position.row} / span ${position.h}`,
  };
};

/**
 * Validate that all cards fit within the grid bounds
 */
export const validateLayout = (config: LayoutConfig): string[] => {
  const errors: string[] = [];
  const { columns } = config.main.grid;

  config.main.sections.forEach((section) => {
    section.cards.forEach((card) => {
      const { col, w } = card.position;
      
      // Check if card exceeds grid width
      if (col + w - 1 > columns) {
        errors.push(`Card ${card.id} exceeds grid width (col: ${col}, width: ${w}, max columns: ${columns})`);
      }
      
      // Check for negative positions
      if (col < 1 || card.position.row < 1) {
        errors.push(`Card ${card.id} has invalid position (col: ${col}, row: ${card.position.row})`);
      }
    });
  });

  return errors;
};

/**
 * Generate CSS grid template for the layout
 */
export const generateGridTemplate = (gridConfig: LayoutConfig['main']['grid']) => {
  return {
    gridTemplateColumns: `repeat(${gridConfig.columns}, 1fr)`,
    gap: `${gridConfig.gutterPx}px`,
    gridAutoRows: `${gridConfig.rowHeightPx}px`,
  };
};

/**
 * Find card by ID across all sections
 */
export const findCardById = (config: LayoutConfig, cardId: string) => {
  for (const section of config.main.sections) {
    const card = section.cards.find(c => c.id === cardId);
    if (card) return { card, section };
  }
  return null;
};

/**
 * Get all cards of a specific type
 */
export const getCardsByType = <T extends LayoutConfig['main']['sections'][0]['cards'][0]>(
  config: LayoutConfig, 
  type: T['type']
): T[] => {
  const cards: T[] = [];
  config.main.sections.forEach(section => {
    section.cards.forEach(card => {
      if (card.type === type) {
        cards.push(card as T);
      }
    });
  });
  return cards;
};
