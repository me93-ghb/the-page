import { mount } from 'svelte';
import App from './App.svelte';
import '@fontsource-variable/alegreya';
import '@fontsource-variable/alegreya/wght-italic.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import '@fontsource/ibm-plex-mono/400-italic.css';
import './app.css';
mount(App, { target: document.getElementById('app')! });
