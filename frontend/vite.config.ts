import {defineConfig, loadEnv} from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
    // Load env file based on `mode` in the current working directory.
    // Set the third parameter to '' to load all env regardless of the
    // `VITE_` prefix.
    const env = loadEnv(mode, process.cwd(), '')
    return {
        plugins: [
            svelte(),
            tailwindcss(),
        ],
        server: {
            proxy: {
                '/api': env.DEVELOPMENT_API_HOST,
                '/swagger': env.DEVELOPMENT_API_HOST,
            }
        }
    };
});
