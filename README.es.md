# Launchtype

*[Read in English](README.md)*

Escribí esta aplicación para lanzar rápidamente comandos (aplicaciones) con o sin argumentos de línea de comandos.

Tengo una aplicación para Mac llamada [Launchbar](https://www.obdev.at/products/launchbar/index.html) que hace esto de forma muy eficiente, permitiéndome ejecutar aplicaciones o páginas web mediante pequeños comandos o abreviaturas.

No me gusta tener el escritorio de Windows desordenado y a veces tengo muchas páginas web distintas con URLs complicadas que acabo guardando en archivos de texto; tenía que buscar el archivo, copiar la dirección al navegador, etc. Con esto se acabó.

Es un lanzador al que se accede pulsando Ctrl+Alt+Espacio (puede que en el futuro lo haga configurable).

Puedes añadir comandos desde la interfaz. Por ejemplo, añadir chrome.exe con una URL como argumento para abrir una web, o añadir tu juego favorito poniendo la ruta del ejecutable para lanzarlo con un comando.

Desde la interfaz también puedes copiar comandos existentes, editarlos y eliminarlos.

Los comandos se guardan en un fichero commands.json (o el que indiques por línea de comandos) que puede editarse con cualquier editor de texto que soporte JSON.

> Antes esto era una aplicación en Python. Ahora está escrita en Rust (interfaz wxWidgets a través de [wxDragon](https://crates.io/crates/wxdragon)), lo que significa un único ejecutable nativo: sin intérprete, sin entorno virtual y sin instalar dependencias en la máquina donde lo ejecutas. Funciona en Windows y en macOS.

## Instalación

Coge la carpeta que produce una compilación (ver más abajo) y déjala donde quieras — Launchtype es portable. En Windows esa carpeta contiene `launchtype.exe`, `prism.dll`, `tolk.dll`, `sounds/` y `locale/`. En macOS es `Launchtype.app`.

Todos tus archivos de datos viven **junto al ejecutable** (junto al paquete `.app` en macOS), así que todo el conjunto puede ir en un pendrive o en tu Dropbox:

`commands.json`, `settings.json`, `timers.json`, `alarms.json`, `clipboard_history.json`, `realtime_history.json`, `snippets/`, `screenshots/`.

No se escribe nada en el registro, en `AppData` ni en `~/Library`.

## Compilar desde el código fuente

Necesitas:

1. **Rust estable** (1.92 o posterior). Instálalo con [rustup](https://rustup.rs); la versión fijada está en `rust-toolchain.toml`.
2. **Un compilador de C++** para wxWidgets: en Windows, las Visual Studio Build Tools con la carga de trabajo "Desarrollo para el escritorio con C++"; en macOS, las herramientas de línea de comandos de Xcode (`xcode-select --install`).
3. **El SDK de voz Prism** (`prism-sdk-vX.Y.Z`), que se usa para hablar por el lector de pantalla. Apunta `PRISM_SDK_DIR` hacia él si no está en la ruta por defecto que hay en `crates/prism-sys/build.rs`.

Después:

```powershell
$env:PRISM_SDK_DIR = "C:\ruta\a\prism-sdk-v0.16.7"
cargo build --release -p launchtype
```

El ejecutable queda en `target/release/launchtype.exe`. Durante el desarrollo también funciona `cargo run -p launchtype` — el script de compilación copia las DLL de Prism junto al binario para que arranque sin más.

Las pruebas se ejecutan con `cargo test`.

### Windows: compilar, desplegar y relanzar

```powershell
pwsh ./scripts/deploy.ps1
```

Compila en modo release, monta `dist/` (ejecutable + DLL de Prism + `sounds/` + `locale/`), cierra la instancia en ejecución, lo copia todo a `%USERPROFILE%\stuff\software\launchtype` y lo vuelve a lanzar. Tus archivos de datos de la carpeta de destino no se tocan nunca.

### macOS: generar el paquete .app

```bash
PRISM_SDK_DIR=/ruta/a/prism-sdk-v0.16.7 ./scripts/bundle-mac.sh
```

Produce `dist/Launchtype.app`, firmado ad-hoc y con `LSUIElement` activado para que viva en segundo plano y se invoque con el atajo global en lugar de aparecer en el Dock. La primera captura de pantalla pedirá el permiso de Grabación de Pantalla.

### Organización del código

| Crate | Qué contiene |
|-------|--------------|
| `crates/launchtype-core` | Modelo de datos, almacenamiento, búsqueda, ajustes, i18n, fuentes de datos en tiempo real — sin interfaz, con pruebas unitarias |
| `crates/launchtype-services` | Efectos: ejecutar comandos, sonidos, portapapeles, capturas, escaneo de Steam, visión por IA, planificadores |
| `crates/launchtype-app` | La interfaz wxDragon, los diálogos, el atajo global y la voz |
| `crates/prism`, `crates/prism-sys` | Envoltorio seguro y bindings del SDK de voz Prism |

Las traducciones son catálogos gettext en `assets/locale/<idioma>/LC_MESSAGES/`. `scripts/compile_catalog.py` compila un `.po` en un `.mo`, y `scripts/check_msgids.py` comprueba que toda cadena traducible del código tenga su entrada.

## Uso

La aplicación admite varios parámetros de línea de comandos:

- `-m, --start-minimized`: Arranca la aplicación minimizada
- `-s, --snippets-on-invoke`: Arranca en modo sustituciones en lugar de modo comandos
- `-q, --quiet`: Desactiva todos los sonidos en esta ejecución
- `-c, --commands [archivo]`: Indica un archivo de comandos personalizado (por defecto: commands.json)
- `-l, --steam-library [ruta]`: Indica una ruta personalizada de la biblioteca de Steam (por defecto: C:\Program Files (x86)\Steam\steamapps)

Una vez añadido un comando desde el botón Añadir de la interfaz, para usarlo puedes:

1. Seleccionarlo de la lista.
2. Escribir su acceso abreviado (si lo tiene) en la caja de texto.
3. Escribir suficientes letras del nombre para que aparezca en la lista y el lector de pantalla lo anuncie.

En modo comandos hay un cuadro combinado "Ordenar comandos por" que permite ordenar la lista por última modificación (lo predeterminado) o por número de usos. La elección se recuerda.

## Ajustes

El botón Ajustes de la interfaz abre un diálogo donde puedes guardar estas preferencias en `settings.json`:

- Habilitar sonidos
- Arrancar minimizado
- Arrancar en modo sustituciones al invocar
- Ruta de la biblioteca de Steam
- Modelo de IA para las descripciones de capturas (Claude Opus, Sonnet o Haiku)

Los parámetros de línea de comandos tienen prioridad sobre estos ajustes durante la ejecución actual (por ejemplo, pasando `-q` se desactivan los sonidos aunque el ajuste esté habilitado, y pasando `-m` se arranca minimizado aunque el ajuste esté desactivado).

## Sustituciones

Las sustituciones son fragmentos de texto que, al escribir su nombre de archivo en la caja de texto, se copian al portapapeles.

Para usarlas hay que crear archivos .txt dentro de la carpeta snippets de la aplicación. El botón "Nueva sustitución" crea uno por ti, y "Abrir carpeta de sustituciones" abre esa carpeta en el explorador de archivos.

El nombre del archivo es el acceso abreviado (sin la extensión .txt) y el contenido es lo que se copia.

Por ejemplo, con un archivo email.txt que contenga mi_email@gmail.com, basta con escribir "email" en la caja y pulsar Intro para tener tu email en el portapapeles.

Para acceder a las sustituciones debes estar en modo sustituciones: escribe un guion (-) en la caja. Desaparecerán los comandos y aparecerán las sustituciones.

Para volver a comandos, escribe un punto (.). En cualquier caso, cada vez que se invoca con Ctrl+Alt+Espacio la aplicación arranca en modo comandos, así que no hace falta hacer nada.

## Historial del portapapeles

El historial del portapapeles se abre escribiendo ? (signo de interrogación) en la caja. Muestra hasta 50 elementos de texto que hayas copiado y se conserva entre reinicios.

Solo funciona con elementos de texto, no con rutas de archivos u otros formatos.

## Lanzador de juegos de Steam

El modo de juegos de Steam se abre escribiendo , (coma) en la caja. Este modo escanea tu biblioteca de Steam en busca de juegos instalados y te permite lanzarlos directamente.

El escáner busca los juegos instalados en la carpeta de la biblioteca de Steam (por defecto: C:\Program Files (x86)\Steam\steamapps) analizando los archivos appmanifest. Puedes indicar una ruta personalizada con el parámetro `-l` o desde el diálogo de Ajustes.

Estando en modo Steam, puedes buscar juegos por nombre con búsqueda difusa igual que con los comandos. Al seleccionar un juego se lanza a través de Steam.

Para volver a comandos, pulsa la tecla punto (.).

## Capturas de pantalla

El modo de capturas de pantalla se abre escribiendo ' (apóstrofo) en la caja. La ventana se oculta antes de capturar, así que Launchtype nunca sale en la imagen. Hay ocho acciones, cada una con un número como acceso abreviado:

1. capturar la ventana activa al portapapeles.
2. capturar toda la pantalla al portapapeles.
3. describir la ventana activa.
4. describir toda la pantalla.
5. explorar las regiones de la ventana activa.
6. explorar las regiones de toda la pantalla.
7. recortar una región concreta de la ventana activa.
8. recortar una región concreta de toda la pantalla.

Las dos primeras simplemente copian el archivo JPEG resultante al portapapeles para que puedas pegarlo en cualquier aplicación que acepte imágenes.

**Describir** envía la captura a una IA y lee en voz alta una descripción escrita para alguien que no puede ver la pantalla.

**Explorar regiones** pide a la IA hasta 8 zonas interesantes de la captura (diálogos, barras de herramientas, áreas de texto, grupos de botones...) y las pone en una lista. Al seleccionar una, la imagen se recorta a esa región y el recorte se copia al portapapeles.

**Recortar una región concreta** usa lo que hayas escrito en la caja de texto como el elemento a buscar — por ejemplo escribe `el botón aceptar` y elige la acción 7. Si la IA lo encuentra, el recorte acaba en el portapapeles; si no, te dice por qué.

Las funciones de IA usan **tu sesión existente de Claude o de ChatGPT**, no una clave de API: primero el token OAuth de Claude Code de `~/.claude/.credentials.json`, y como alternativa el token de la CLI de Codex en `~/.codex/auth.json`. Si no hay ninguno, la aplicación te lo dice. El modelo que se usa con Claude se elige en el diálogo de Ajustes.

Para volver a comandos, pulsa la tecla punto (.).

## Temporizadores

El modo de temporizadores se abre escribiendo `[` (corchete izquierdo) en la caja. Los temporizadores cuentan atrás durante unos minutos y luego te avisan.

Añade uno con el botón Añadir. El diálogo permite configurar:

- Un **título** y una **descripción** (que se anuncian por el lector de pantalla al dispararse).
- El número de **minutos** de la cuenta atrás.
- Una casilla de **repetición**.
- Un **archivo de sonido** propio (cualquier .wav de tu sistema, elegido con Examinar). Si no se indica ninguno, se usa el sonido integrado.

Los temporizadores aparecen en la lista con su estado actual:

- Los **no repetitivos** aparecen como `parado` hasta que se inician. Al ejecutarlos (Intro o Alt+R) empieza la cuenta atrás; ejecutarlos de nuevo mientras cuentan **reinicia** el temporizador. Se disparan una vez y se detienen.
- Los **repetitivos** se disparan cada X minutos hasta que se desactivan. Vienen **activados** por defecto, y ejecutarlos (Intro o Alt+R) los **alterna** entre activado y desactivado.

Para volver a comandos, pulsa la tecla punto (.).

## Alarmas

El modo de alarmas se abre escribiendo `]` (corchete derecho) en la caja. Las alarmas se disparan una vez al día a una hora concreta en formato de 24 horas.

Añade una con el botón Añadir. El diálogo permite configurar:

- Un **título** y una **descripción** (que se anuncian por el lector de pantalla al dispararse).
- La **hora** (0-23) y los **minutos** (0-59).
- Un **archivo de sonido** propio (cualquier .wav de tu sistema, elegido con Examinar). Si no se indica ninguno, se usa el sonido integrado.

Las alarmas aparecen en la lista con su hora y si están `activada` o `desactivada`. Ejecuta una alarma (Intro o Alt+R) para alternar su estado.

Para volver a comandos, pulsa la tecla punto (.).

## Notas de Notebrook

El modo Notebrook se abre escribiendo `#` (almohadilla) en la caja. Permite mandar una nota rápida a tu cuenta de [Notebrook](https://notebrook.com) sin salir del lanzador.

Escribe la nota y pulsa Intro (o Alt+R). La nota se publica en un canal llamado **feeds**, que se crea automáticamente la primera vez si no existe. Se recortan los espacios sobrantes y no se envía nada si el campo está vacío.

La primera vez que envíes una nota se te pedirán la **URL del servidor** y el **token** en un diálogo de dos campos. Se guardan localmente en `settings.json` (que está en .gitignore, así que nunca se sube al repositorio) y se reutilizan después. Si el token deja de ser válido, las credenciales guardadas se borran y se te volverán a pedir en el siguiente intento.

Al terminar, la aplicación anuncia si la nota se envió o, si algo falló, el motivo (error de red, URL incorrecta, token no autorizado, etc.).

Para volver a comandos, pulsa la tecla punto (.).

## Datos en tiempo real

El modo de datos en tiempo real se abre escribiendo `+` (signo más) en la caja. Ofrece valores en directo obtenidos de APIs públicas gratuitas en el momento de seleccionarlos:

- `btc`: precio del bitcoin en euros (CoinGecko)
- `eth`: precio del ethereum en euros (CoinGecko)
- `usd`: cuánto valen 1000 euros en dólares estadounidenses (tipos del Banco Central Europeo)
- `oil`: precio del barril de petróleo brent (Yahoo Finance)
- `gold`: precio de la onza de oro (Yahoo Finance)
- `ibex`: índice bursátil IBEX 35 (Yahoo Finance)
- `w`: el tiempo actual en tu ubicación (geolocalizada por IP, datos de Open-Meteo)
- `news`: titulares de portada de El País
- `cat`: titulares de Catalunya de La Vanguardia
- `vila`: titulares en catalán de VilaWeb
- `bbc`: titulares internacionales de la BBC
- `cc`: tu uso de la suscripción de Claude (límites de sesión y semanales, leídos de la sesión local de Claude Code — no hace falta clave de API)
- `t`: las temperaturas, velocidades de ventilador y GPU de tu ordenador (ver [Temperaturas del ordenador](#temperaturas-del-ordenador) más abajo)

Pulsa Intro (o Alt+R) sobre un elemento: la aplicación anuncia "Obteniendo..." y a continuación lee el valor en directo por el lector de pantalla en cuanto llega. La ventana permanece abierta para que puedas consultar varios valores seguidos. Si una consulta falla (sin red, servicio caído), se anuncia el motivo.

Todas las fuentes en línea son gratuitas y no requieren clave de API ni cuenta.

Para volver a comandos, pulsa la tecla punto (.).

### Temperaturas del ordenador

El elemento `t` lee los sensores de hardware de forma local (no se envía nada por la red) y lee en voz alta una sola frase con la temperatura de la CPU/sistema, la temperatura de la GPU, las velocidades de los ventiladores y la carga de la GPU — por ejemplo: *"Temperaturas: CPU a 42 grados. GPU NVIDIA GeForce RTX 5070 a 48 grados, ventilador al 30 por ciento, carga al 5 por ciento. Ventilador de CPU a 1200 rpm."*

Reúne lo que tu máquina exponga, de varias fuentes, e informa solo de lo que tenga éxito:

- **GPU NVIDIA** — se lee con `nvidia-smi`, que se instala con el controlador de NVIDIA. Da el nombre de la GPU, la temperatura, el porcentaje del ventilador y la carga. Funciona sin más en cualquier equipo con una tarjeta NVIDIA; no hace falta software adicional.
- **Cualquier GPU** — si no hay controlador de NVIDIA, el nombre del adaptador se lee de Windows para que al menos obtengas "GPU &lt;nombre&gt;".
- **Temperatura de la CPU y rpm de los ventiladores** — Windows **no** expone estos datos a los programas normales. Para leerlos necesitas instalar y ejecutar **LibreHardwareMonitor** con su servidor web activado (ver más abajo). Cuando está en marcha, Launchtype recoge sus lecturas automáticamente; cuando no lo está, la frase de temperaturas simplemente omite esas partes.

#### Instalar LibreHardwareMonitor (opcional, para temperatura de CPU y ventiladores)

LibreHardwareMonitor es un monitor de hardware gratuito y de código abierto. Launchtype no lo incluye ni lo requiere — instálalo solo si quieres temperatura de CPU y rpm de ventiladores en el elemento `t`.

1. **Instálalo.** Lo más fácil es [winget](https://learn.microsoft.com/windows/package-manager/) desde una terminal:

   ```powershell
   winget install --id LibreHardwareMonitor.LibreHardwareMonitor -e
   ```

   O descarga el ZIP manualmente desde la [página de versiones de LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases) y extráelo donde quieras.

2. **Ejecútalo como administrador.** Haz clic derecho en `LibreHardwareMonitor.exe` → *Ejecutar como administrador*. Se necesitan permisos de administrador para que cargue su controlador de kernel y lea las temperaturas de la CPU y las velocidades de los ventiladores.

3. **Activa su servidor web.** En el menú *Options*, abre *Remote Web Server* y pulsa *Run* (el puerto por defecto es 8085). LibreHardwareMonitor sirve entonces todos los sensores como JSON en `http://localhost:8085/data.json`, que es lo que Launchtype lee de forma local — nada sale de tu equipo. El ajuste se recuerda, así que el servidor vuelve a arrancar automáticamente la próxima vez.

4. **Déjalo abierto en segundo plano.** Las lecturas solo están disponibles mientras LibreHardwareMonitor esté en ejecución. En su menú *Options* también puedes activar, para que esté siempre listo tras iniciar sesión:
   - *Run On Windows Startup* (arrancar con Windows)
   - *Start Minimized* (arrancar minimizado)
   - *Minimize To Tray* (minimizar a la bandeja) y *Minimize On Close* (minimizar al cerrar)

OpenHardwareMonitor (el proyecto anterior del que deriva) también funciona — activa su *Remote Web Server* (mismo puerto por defecto 8085) y Launchtype lo leerá igualmente.

## Estadísticas de uso

El modo de estadísticas se abre escribiendo `!` (signo de exclamación) en la caja. Es una lista de solo lectura que muestra cuántos comandos has ejecutado en total, tus 10 comandos más usados y los 10 menos usados.

Para volver a comandos, pulsa la tecla punto (.).

## Ejecutar como administrador

Al añadir o editar un comando puedes marcar la casilla "Ejecutar como administrador". El comando se lanzará con privilegios elevados (aparecerá el cuadro de UAC al ejecutarlo).

## Copiar los argumentos de un comando

Selecciona un comando en la lista y pulsa `Alt+O` (o usa el botón Copiar Argumentos) para copiar los argumentos de ese comando al portapapeles. Muy útil para comandos que almacenan URLs o cadenas largas que quieres obtener rápidamente.

## Resumen de modos

La aplicación tiene varios modos, cada uno accesible escribiendo un carácter especial en la caja:

| Carácter | Modo | Descripción |
|----------|------|-------------|
| (por defecto) | Comandos | Ejecutar comandos y aplicaciones guardados |
| `-` | Sustituciones | Copiar fragmentos de texto al portapapeles |
| `?` | Portapapeles | Acceder al historial del portapapeles |
| `,` | Steam | Lanzar juegos de Steam instalados |
| `'` | Capturas | Capturar, describir o recortar una ventana o la pantalla completa |
| `[` | Temporizadores | Cuenta atrás de X minutos (una vez o repetitiva) |
| `]` | Alarmas | Se disparan a una hora del día (formato 24 horas) |
| `#` | Notebrook | Publicar una nota rápida en tu Notebrook |
| `+` | Datos en tiempo real | Leer en voz alta precios, tiempo, titulares y temperaturas del ordenador |
| `!` | Estadísticas | Comandos más y menos usados |
| `.` | (cualquier modo) | Volver al modo Comandos |

## Retroalimentación de audio

La aplicación emite sonidos ante distintas acciones:

- Sonido de arranque al iniciar la aplicación
- Sonidos de mostrar/ocultar al alternar la ventana
- Sonido de coincidencia cuando se encuentra un acceso abreviado exacto
- Sonido al escribir cuando cambian los resultados de búsqueda
- Sonido al ejecutar un comando o lanzar un juego
- Sonido al copiar una sustitución o un elemento del portapapeles

Los sonidos se pueden desactivar desde el diálogo de Ajustes o lanzando la aplicación con `-q`.

## Accesibilidad

La aplicación está pensada con la accesibilidad en mente, en especial para usuarios de lectores de pantalla:

- Todos los cambios de interfaz se anuncian por el lector de pantalla (mediante la biblioteca de voz Prism, que habla con NVDA, JAWS y VoiceOver)
- El primer resultado de búsqueda se lee automáticamente
- Interfaz totalmente manejable por teclado (no hace falta ratón)
- Retroalimentación sonora en todas las interacciones

## Problemas conocidos

La apariencia visual puede no ser la ideal. Soy ciego y no puedo depurar la interfaz.
Alternativa: abre un PR y ayúdame a mejorarla ;)

## TODO

 1. Hacer configurable el atajo global.
 2. Publicar compilaciones firmadas y notarizadas para macOS.
 3. Más idiomas además de inglés y español.
