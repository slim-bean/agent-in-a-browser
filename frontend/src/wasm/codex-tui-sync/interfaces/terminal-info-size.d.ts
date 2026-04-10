/** @module Interface terminal:info/size@0.1.0 **/
export function getTerminalSize(): TerminalDimensions;
export interface TerminalDimensions {
  cols: number,
  rows: number,
}
