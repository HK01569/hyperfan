export interface AppConfig {
  name: string;
  icon: string;
  theme: {
    primaryColor: string;
    background: string;
    card: string;
    text: string;
  };
}

export interface HeaderItem {
  type: 'icon-button' | 'app-icon' | 'title';
  icon?: string;
  text?: string;
  tooltip?: string;
}

export interface HeaderConfig {
  left: HeaderItem[];
  right: HeaderItem[];
  heightPx: number;
}

export interface SidebarItem {
  id: string;
  icon: string;
  label: string;
  active?: boolean;
}

export interface SidebarConfig {
  widthPx: number;
  items: SidebarItem[];
}

export interface GridPosition {
  row: number;
  col: number;
  w: number;
  h: number;
}

export interface FanTuning {
  stepUp: string;
  stepDown: string;
  startPercent: string;
  stopPercent: string;
  offset: string;
  minimumPercent: string;
}

export interface FanControl {
  type: 'slider' | 'toggle';
  value: string;
}

export interface FanReadouts {
  percent?: string;
  rpm?: string;
}

export interface TemperatureSource {
  id: string;
  label: string;
  currentTemp: string;
}

export interface FanControlCard {
  id: string;
  type: 'fan-control';
  title: string;
  curve: string;
  statusBadge: string;
  readouts: FanReadouts;
  temperatureSource?: {
    selected: string;
    available: TemperatureSource[];
  };
  hysteresis?: {
    value: number;
    min: number;
    max: number;
    step: number;
  };
  tuning?: FanTuning;
  controls?: FanControl[];
  actionsMenu: boolean;
  position: GridPosition;
}

export interface GraphSeries {
  label: string;
  style: 'line';
  trend: 'rising' | 'rising-from-zero';
}

export interface Graph {
  xAxis: string;
  yAxis: string;
  series: GraphSeries[];
}

export interface CurveCard {
  id: string;
  type: 'curve-card';
  title: string;
  temperatureSource: string;
  graph: Graph;
  readouts: FanReadouts;
  cta: string;
  position: GridPosition;
}

export interface MixerConfig {
  availableFanCurves: string[];
  active: string[];
}

export interface MixerCard {
  id: string;
  type: 'mixer-card';
  title: string;
  function: 'Max' | 'Min' | 'Average';
  mixing: MixerConfig;
  readouts: FanReadouts;
  position: GridPosition;
}

export interface SensorCard {
  id: string;
  type: 'sensor-card';
  title: string;
  currentTemp: string;
  minTemp: string;
  maxTemp: string;
  status: 'Normal' | 'Warning' | 'Critical';
  position: GridPosition;
}

export type Card = FanControlCard | CurveCard | MixerCard | SensorCard;

export interface Badge {
  text: string;
  context: string;
}

export interface Section {
  id: string;
  title: string;
  badges?: Badge[];
  cards: Card[];
}

export interface GridConfig {
  columns: number;
  gutterPx: number;
  rowHeightPx: number;
}

export interface MainConfig {
  containerPaddingPx: number;
  grid: GridConfig;
  sections: Section[];
}

export interface LayoutConfig {
  app: AppConfig;
  header: HeaderConfig;
  sidebar: SidebarConfig;
  main: MainConfig;
  assumptions: string[];
}
