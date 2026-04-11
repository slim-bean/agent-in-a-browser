import { describe, expect, it } from 'vitest';
import { mapShellPathToPyodide } from './pyodide-loader';

describe('mapShellPathToPyodide', () => {
    it('absolute root stays /', () => {
        expect(mapShellPathToPyodide('/')).toBe('/');
    });

    it('absolute path passes through unchanged', () => {
        expect(mapShellPathToPyodide('/script.py')).toBe('/script.py');
    });

    it('absolute nested path passes through unchanged', () => {
        expect(mapShellPathToPyodide('/project/main.py')).toBe('/project/main.py');
    });

    it('relative path resolves against cwd', () => {
        expect(mapShellPathToPyodide('main.py', '/project')).toBe('/project/main.py');
    });

    it('relative path resolves against root cwd', () => {
        expect(mapShellPathToPyodide('script.py', '/')).toBe('/script.py');
    });

    it('/home/user/script.py is NOT rewritten — treated as a literal path', () => {
        expect(mapShellPathToPyodide('/home/user/script.py')).toBe('/home/user/script.py');
    });

    it('normalizes double slashes', () => {
        expect(mapShellPathToPyodide('//foo//bar.py')).toBe('/foo/bar.py');
    });

    it('strips trailing slash', () => {
        expect(mapShellPathToPyodide('/project/')).toBe('/project');
    });
});
