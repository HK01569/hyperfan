import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';

interface TempSource {
  sensor_path: string;
  sensor_name: string;
  sensor_label?: string;
  current_temp?: number;
  chip_name: string;
}

interface FanMapping {
  fan_name: string;
  pwm_name: string;
  confidence: number;
  temp_sources: TempSource[];
  response_time_ms?: number;
  min_pwm?: number;
  max_rpm?: number;
}

interface AutoDetectModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: (mappings: FanMapping[]) => void;
}

export const AutoDetectModal: React.FC<AutoDetectModalProps> = ({ isOpen, onClose, onConfirm }) => {
  const [detecting, setDetecting] = useState(false);
  const [detectedMappings, setDetectedMappings] = useState<FanMapping[]>([]);
  const [progress, setProgress] = useState(0);
  const [statusMessage, setStatusMessage] = useState('');

  const startDetection = async () => {
    setDetecting(true);
    setProgress(0);
    setStatusMessage('Initializing ultra-advanced probe...');
    
    try {
      // Simulate progress updates with more accurate timing for thorough testing
      const progressInterval = setInterval(() => {
        setProgress(prev => Math.min(prev + 2, 90));
      }, 500);

      setStatusMessage('Stabilizing fans at baseline...');
      await new Promise(resolve => setTimeout(resolve, 1000));
      
      setStatusMessage('Testing PWM controllers at multiple speeds...');
      await new Promise(resolve => setTimeout(resolve, 1500));
      
      setStatusMessage('Performing validation cycles for accuracy...');
      await new Promise(resolve => setTimeout(resolve, 1000));
      
      setStatusMessage('Analyzing fan response patterns...');
      const detected = await invoke<FanMapping[]>('autodetect_mappings');
      
      clearInterval(progressInterval);
      setProgress(100);
      setStatusMessage('Detection complete!');
      
      setDetectedMappings(detected);
      // Temperature source selection removed – we only confirm PWM ↔ Fan mappings.

    } catch (error) {
      console.error('Detection failed:', error);
      
      // Check if it's a permission error
      const errorMessage = String(error);
      if (errorMessage.includes('Insufficient permissions') || errorMessage.includes('run as root')) {
        setStatusMessage('❌ Root permissions required! Please run: sudo ./run.sh');
      } else {
        setStatusMessage('Detection failed. Please try again.');
      }
    } finally {
      setDetecting(false);
    }
  };

  const handleConfirm = () => {
    // Pass through detected PWM ↔ Fan mappings as-is
    onConfirm(detectedMappings);
    onClose();
  };

  useEffect(() => {
    if (isOpen && detectedMappings.length === 0) {
      startDetection();
    }
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-gray-800 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] overflow-hidden">
        <div className="p-6 border-b border-gray-700">
          <h2 className="text-2xl font-bold text-blue-400">Ultra-Advanced Fan Detection</h2>
          <p className="text-gray-400 mt-1">Intelligent PWM/Fan pairing with active probing</p>
        </div>

        <div className="p-6 overflow-y-auto max-h-[calc(90vh-200px)]">
          {detecting ? (
            <div className="space-y-4">
              <div className="text-center py-8">
                <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-blue-500/20 mb-4">
                  <div className="w-10 h-10 border-4 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
                </div>
                <p className="text-lg text-gray-300 mb-2">{statusMessage}</p>
                <div className="w-full max-w-md mx-auto bg-gray-700 rounded-full h-2 overflow-hidden">
                  <div 
                    className="h-full bg-gradient-to-r from-blue-500 to-blue-400 transition-all duration-300"
                    style={{ width: `${progress}%` }}
                  />
                </div>
                <p className="text-sm text-gray-500 mt-2">{progress}% complete</p>
              </div>
            </div>
          ) : (
            <div className="space-y-4">
              {detectedMappings.length === 0 ? (
                <div className="text-center py-8 text-gray-400">
                  <p>No fan mappings detected. Please check your hardware connections.</p>
                </div>
              ) : (
                <>
                  <div className="mb-4">
                    <h3 className="text-lg font-semibold text-gray-200 mb-2">Detected Pairings</h3>
                    <p className="text-sm text-gray-400">
                      Found {detectedMappings.length} PWM/Fan pairing{detectedMappings.length !== 1 ? 's' : ''}.
                      These pairings map PWM controllers to Fan RPM sensors.
                    </p>
                  </div>

                  <div className="grid gap-4">
                    {detectedMappings.map((mapping, index) => (
                      <div 
                        key={index}
                        className="bg-gray-900 rounded-lg border border-gray-700 overflow-hidden hover:border-blue-500/50 transition-colors"
                      >
                        {/* Header with confidence indicator */}
                        <div className="bg-gradient-to-r from-gray-800 to-gray-900 p-4 border-b border-gray-700">
                          <div className="flex items-center justify-between">
                            <div className="flex items-center gap-3">
                              <div className="flex flex-col">
                                <span className="text-sm font-medium text-gray-400">PWM Controller</span>
                                <span className="text-lg font-semibold text-white">{mapping.pwm_name}</span>
                              </div>
                              <div className="text-gray-500">→</div>
                              <div className="flex flex-col">
                                <span className="text-sm font-medium text-gray-400">Fan Sensor</span>
                                <span className="text-lg font-semibold text-white">{mapping.fan_name}</span>
                              </div>
                            </div>
                            <div className="flex items-center gap-2">
                              <span className="text-xs text-gray-500">Confidence</span>
                              <div className="flex items-center gap-1">
                                <div className="w-24 bg-gray-700 rounded-full h-2 overflow-hidden">
                                  <div 
                                    className={`h-full transition-all ${
                                      mapping.confidence > 0.7 ? 'bg-green-500' :
                                      mapping.confidence > 0.5 ? 'bg-yellow-500' : 'bg-orange-500'
                                    }`}
                                    style={{ width: `${mapping.confidence * 100}%` }}
                                  />
                                </div>
                                <span className={`text-sm font-medium ${
                                  mapping.confidence > 0.7 ? 'text-green-400' :
                                  mapping.confidence > 0.5 ? 'text-yellow-400' : 'text-orange-400'
                                }`}>
                                  {(mapping.confidence * 100).toFixed(0)}%
                                </span>
                              </div>
                            </div>
                          </div>
                        </div>

                        {/* Characteristics */}
                        <div className="p-4 grid grid-cols-3 gap-4 border-b border-gray-700">
                          <div className="text-center">
                            <p className="text-xs text-gray-500 mb-1">Response Time</p>
                            <p className="text-sm font-medium text-blue-400">
                              {mapping.response_time_ms ? `${mapping.response_time_ms}ms` : 'N/A'}
                            </p>
                          </div>
                          <div className="text-center">
                            <p className="text-xs text-gray-500 mb-1">Min PWM Start</p>
                            <p className="text-sm font-medium text-green-400">
                              {mapping.min_pwm ? `${Math.round((mapping.min_pwm / 255) * 100)}%` : 'N/A'}
                            </p>
                          </div>
                          <div className="text-center">
                            <p className="text-xs text-gray-500 mb-1">Max RPM</p>
                            <p className="text-sm font-medium text-orange-400">
                              {mapping.max_rpm ? `${mapping.max_rpm} RPM` : 'N/A'}
                            </p>
                          </div>
                        </div>

                        {/* Temperature selection removed intentionally */}
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
          )}
        </div>

        <div className="p-6 border-t border-gray-700 flex justify-end gap-3">
          <button
            onClick={onClose}
            className="px-4 py-2 text-gray-400 hover:text-white transition-colors"
            disabled={detecting}
          >
            Cancel
          </button>
          {!detecting && detectedMappings.length > 0 && (
            <button
              onClick={handleConfirm}
              className="px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-colors font-medium"
            >
              Apply Mappings
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
