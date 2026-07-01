# Launchtype

*[Read in English](README.md)*

Escribí esta aplicación para lanzar rápidamente comandos (aplicaciones) con o sin argumentos de línea de comandos en Windows.

Tengo una aplicación para Mac llamada [Launchbar](https://www.obdev.at/products/launchbar/index.html) que hace esto de forma muy eficiente, permitiéndome ejecutar aplicaciones o páginas web mediante pequeños comandos o abreviaturas.

No me gusta tener el escritorio de Windows desordenado y a veces tengo muchas páginas web distintas con URLs complicadas que acabo guardando en archivos de texto; tenía que buscar el archivo, copiar la dirección al navegador, etc. Con esto se acabó.

Es un lanzador al que se accede pulsando Ctrl+Alt+Espacio (puede que en el futuro lo haga configurable).

Puedes añadir comandos desde la interfaz. Por ejemplo, añadir chrome.exe con una URL como argumento para abrir una web, o añadir tu juego favorito poniendo la ruta del ejecutable para lanzarlo con un comando.

Desde la interfaz también puedes copiar comandos existentes, editarlos y eliminarlos.

Los comandos se guardan en un fichero commands.json (o el que indiques por línea de comandos) que puede editarse con cualquier editor de texto que soporte JSON.

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

## Ajustes

El botón Ajustes de la interfaz abre un diálogo donde puedes guardar estas preferencias en `settings.json`:

- Habilitar sonidos
- Arrancar minimizado
- Arrancar en modo sustituciones al invocar
- Ruta de la biblioteca de Steam

Los parámetros de línea de comandos tienen prioridad sobre estos ajustes durante la ejecución actual (por ejemplo, pasando `-q` se desactivan los sonidos aunque el ajuste esté habilitado, y pasando `-m` se arranca minimizado aunque el ajuste esté desactivado).

## Sustituciones

Las sustituciones son fragmentos de texto que, al escribir su nombre de archivo en la caja de texto, se copian al portapapeles.

Para usarlas hay que crear archivos .txt dentro de la carpeta snippets de la aplicación.

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

El modo de capturas de pantalla se abre escribiendo ' (apóstrofo) en la caja. Hay dos acciones disponibles:

- Acceso `w`: captura la ventana activa.
- Acceso `s`: captura toda la pantalla.

Al seleccionar una, el archivo JPEG resultante se copia al portapapeles para que puedas pegarlo en cualquier aplicación que acepte imágenes.

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

## Ejecutar como administrador

Al añadir o editar un comando puedes marcar la casilla "Ejecutar como administrador". El comando se lanzará con privilegios elevados (aparecerá el cuadro de UAC al ejecutarlo).

## Copiar los argumentos de un comando

Selecciona un comando en la lista y pulsa `Ctrl+C` (o usa el botón Copiar Argumentos) para copiar los argumentos de ese comando al portapapeles. Muy útil para comandos que almacenan URLs o cadenas largas que quieres obtener rápidamente.

## Resumen de modos

La aplicación tiene varios modos, cada uno accesible escribiendo un carácter especial en la caja:

| Carácter | Modo | Descripción |
|----------|------|-------------|
| (por defecto) | Comandos | Ejecutar comandos y aplicaciones guardados |
| `-` | Sustituciones | Copiar fragmentos de texto al portapapeles |
| `?` | Portapapeles | Acceder al historial del portapapeles |
| `,` | Steam | Lanzar juegos de Steam instalados |
| `'` | Capturas | Capturar ventana o pantalla completa al portapapeles |
| `+` | Datos en tiempo real | Leer en voz alta precios, tiempo y titulares en directo |
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

- Todos los cambios de interfaz se anuncian por el lector de pantalla (usando la biblioteca accessible_output2)
- El primer resultado de búsqueda se lee automáticamente
- Interfaz totalmente manejable por teclado (no hace falta ratón)
- Retroalimentación sonora en todas las interacciones

## Problemas conocidos

Alt+F4 cierra la aplicación y hay que volver a lanzarla (pendiente de arreglar).
Alternativa: usa Ctrl+Alt+Espacio para ocultar la ventana, o lanza un comando. Al lanzar un comando la ventana se oculta.

La apariencia visual puede no ser la ideal. Soy ciego y no puedo depurar la interfaz.
Alternativa: abre un PR y ayúdame a mejorarla ;)

## TODO

 1. Encontrar la manera de evitar que Alt+F4 cierre la ventana.
 2. Encontrar la manera de reproducir audio en Windows.
 3. Asegurarse de que las dependencias en pyproject.toml estén bien configuradas.
 4. Compilarlo como ejecutable para Windows.
 5. Ajustar el método de búsqueda por difflib si procede.
 6. Refactorizar el manejo de comandos y sustituciones en la interfaz.
