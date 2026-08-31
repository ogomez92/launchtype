# Launchtype

*[Read in English](README.md)*

Escribí esta aplicación para lanzar rápidamente comandos (aplicaciones) con o sin argumentos de línea de comandos.

Tengo una aplicación para Mac llamada [Launchbar](https://www.obdev.at/products/launchbar/index.html) que hace esto de forma muy eficiente, permitiéndome ejecutar aplicaciones o páginas web mediante pequeños comandos o abreviaturas.

No me gusta tener el escritorio de Windows desordenado y a veces tengo muchas páginas web distintas con URLs complicadas que acabo guardando en archivos de texto; tenía que buscar el archivo, copiar la dirección al navegador, etc. Con esto se acabó.

Es un lanzador al que se accede pulsando Ctrl+Alt+Espacio, o Ctrl+Cmd+Espacio en Mac (puede que en el futuro lo haga configurable).

Puedes añadir comandos desde la interfaz. Por ejemplo, añadir chrome.exe con una URL como argumento para abrir una web, o añadir tu juego favorito poniendo la ruta del ejecutable para lanzarlo con un comando.

Desde la interfaz también puedes copiar comandos existentes, editarlos y eliminarlos.

Los comandos se guardan en un fichero commands.json (o el que indiques por línea de comandos) que puede editarse con cualquier editor de texto que soporte JSON.

> Antes esto era una aplicación en Python. Ahora está escrita en Rust (interfaz wxWidgets a través de [wxDragon](https://crates.io/crates/wxdragon)), lo que significa un único ejecutable nativo: sin intérprete, sin entorno virtual y sin instalar dependencias en la máquina donde lo ejecutas. Funciona en Windows y en macOS.

## Instalación

Coge la carpeta que produce una compilación (ver más abajo) y déjala donde quieras — Launchtype es portable. En Windows esa carpeta contiene `launchtype.exe`, `prism.dll`, `sounds/` y `locale/`. En macOS es `Launchtype.app`.

Todos tus archivos de datos viven **junto al ejecutable** (junto al paquete `.app` en macOS), así que todo el conjunto puede ir en un pendrive o en tu Dropbox:

`commands.json`, `settings.json`, `timers.json`, `alarms.json`, `clipboard_history.json`, `realtime_history.json`, `snippets/`, `screenshots/`.

No se escribe nada en el registro, en `AppData` ni en `~/Library`.

## Compilar desde el código fuente

Necesitas:

1. **Rust estable** (1.92 o posterior). Instálalo con [rustup](https://rustup.rs); la versión fijada está en `rust-toolchain.toml`.
2. **Un compilador de C++** para wxWidgets: en Windows, las Visual Studio Build Tools con la carga de trabajo "Desarrollo para el escritorio con C++"; en macOS, las herramientas de línea de comandos de Xcode (`xcode-select --install`).
El SDK de voz Prism, que se usa para hablar por el lector de pantalla, no necesita ninguna preparación: las porciones de Windows y macOS con las que se enlaza están incluidas en `vendor/prism-sdk/` (~6 MB) y `crates/prism-sys/build.rs` las usa por defecto. Apunta `PRISM_SDK_DIR` a un `prism-sdk-vX.Y.Z` completo solo si quieres compilar contra otra versión o para Linux.

Después:

```bash
cargo build --release -p launchtype
```

El ejecutable queda en `target/release/launchtype` (`.exe` en Windows). Durante el desarrollo también funciona `cargo run -p launchtype` — en Windows el script de compilación copia las DLL de Prism junto al binario para que arranque sin más.

Las pruebas se ejecutan con `cargo test`.

### Windows: compilar, desplegar y relanzar

```powershell
pwsh ./scripts/deploy.ps1
```

Compila en modo release, monta `dist/` (ejecutable + DLL de Prism + `sounds/` + `locale/`), cierra la instancia en ejecución, lo copia todo a `%USERPROFILE%\stuff\software\launchtype` y lo vuelve a lanzar. Tus archivos de datos de la carpeta de destino no se tocan nunca.

### macOS: generar el paquete .app

```bash
./scripts/bundle-mac.sh
```

Produce `dist/Launchtype.app`, firmado ad-hoc y con `LSUIElement` activado para que viva en segundo plano y se invoque con el atajo global en lugar de aparecer en el Dock. La primera captura de pantalla pedirá el permiso de Grabación de Pantalla.

El paquete se compila solo para la arquitectura de la máquina. `libprism.a` es universal, así que se puede generar una app universal compilando `aarch64-apple-darwin` y `x86_64-apple-darwin` y uniendo los dos binarios con `lipo`; el script no lo hace todavía.

### Organización del código

| Crate | Qué contiene |
|-------|--------------|
| `crates/launchtype-core` | Modelo de datos, almacenamiento, búsqueda, ajustes, i18n, fuentes de datos en tiempo real — sin interfaz, con pruebas unitarias |
| `crates/launchtype-services` | Efectos: ejecutar comandos, sonidos, portapapeles, capturas, escaneo de Steam y de las aplicaciones instaladas, visión por IA, planificadores |
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

## Rutas portables (variables)

Launchtype está pensado para viajar: copia la carpeta a otro equipo (o a un Mac) y sigue funcionando. Normalmente hay dos cosas que lo impiden, y ambas se resuelven con variables que puedes poner en la ruta o en los argumentos de un comando.

Una variable se escribe `{{nombre}}` y cada equipo la resuelve por su cuenta. Las llaves dobles son intencionadas: `%nombre%` sería ambiguo con las URL codificadas en porcentaje (`?sex%5B%5D=female`), habituales en los argumentos de comandos reales.

### Variables de carpetas

| Variable | Windows | macOS |
| --- | --- | --- |
| `{{home}}` | `C:\Users\tu_usuario` | `/Users/tu_usuario` |
| `{{desktop}}`, `{{documents}}`, `{{downloads}}` | tus carpetas Escritorio / Documentos / Descargas | las mismas carpetas |
| `{{music}}`, `{{pictures}}`, `{{videos}}` | tus carpetas Música / Imágenes / Vídeos | las mismas carpetas |
| `{{onedrive}}` | tu carpeta OneDrive | tu carpeta OneDrive |
| `{{appdata}}` | `%APPDATA%` (itinerante) | `~/Library/Application Support` |
| `{{localappdata}}` | `%LOCALAPPDATA%` | `~/Library/Application Support` |
| `{{programfiles}}` | `C:\Program Files` | `/Applications` |
| `{{programfiles86}}` | `C:\Program Files (x86)` | `/Applications` |
| `{{programdata}}` | `C:\ProgramData` | `/Library/Application Support` |
| `{{temp}}` | la carpeta temporal | la carpeta temporal |
| `{{launchtype}}` | la carpeta desde la que se ejecuta Launchtype | la misma |
| `{{username}}` | tu nombre de usuario por sí solo, para argumentos | el mismo |

Las barras invertidas de Windows se traducen automáticamente, así que `{{home}}\stuff\notas.txt` escrito en Windows abre `/Users/tu_usuario/stuff/notas.txt` en un Mac.

### Variables de navegadores

`{{browser}}` es lo que use tu sistema operativo para abrir un enlace, así que un comando con `{{browser}}` como ruta y una URL como argumento funciona en cualquier sitio.

`{{chrome}}`, `{{firefox}}`, `{{edge}}`, `{{brave}}`, `{{vivaldi}}`, `{{opera}}` y `{{safari}}` nombran un navegador concreto, para que puedas mantener unos enlaces en un navegador y otros en otro. Cada uno se busca en las ubicaciones de instalación habituales de la plataforma actual y **recurre a `{{browser}}` cuando ese navegador no está instalado**: un comando con `{{chrome}}` sigue abriéndose en un Mac que solo tenga Safari, en lugar de fallar.

### La variable de consulta

`{{query}}` es la única variable que ningún equipo puede resolver: Launchtype te la pide *a ti*, cada vez que se ejecuta el comando. Convierte un comando en una plantilla, de modo que un solo comando guardado cubre todo lo que quieras buscar, en lugar de un comando por búsqueda.

```
Ruta:       {{browser}}
Argumentos: https://www.google.com/search?q={{query}}
Nombre:     google
```

Al ejecutarlo la ventana se queda abierta: el campo de entrada pasa a pedirte las palabras de la búsqueda en vez de buscar entre tus comandos, y tu lector de pantalla dice "Parámetro de consulta 1" acompañado de su propio sonido. Escribe lo que buscas, pulsa Intro y el navegador abre los resultados. Mientras escribes no se filtra nada (el texto es la respuesta, no una búsqueda), así que una consulta puede empezar por `?`, `@` o cualquier otro carácter de modo sin cambiar de modo, y suena un sonido de escritura distinto en cada pulsación para que oigas cuál de las dos cosas está haciendo el campo.

Un comando puede llevar tantas como quiera, tanto en la ruta como en los argumentos. Se piden en el orden en que aparecen, primero la ruta, anunciadas como "Parámetro de consulta 1", "Parámetro de consulta 2", etc., y el comando se lanza en cuanto entra la última. Escape se sale sin ejecutar nada.

Las respuestas que caen dentro de una URL se codifican en porcentaje, así que sobreviven los espacios, los acentos, los ampersands y las comas: buscar `gatos, perros` abre una única búsqueda de `gatos, perros` en lugar de dos argumentos. En cualquier otro sitio la respuesta pasa tal cual se escribió, porque un `{{query}}` que hace de nombre de archivo tiene que seguir siendo un nombre de archivo:

```
Ruta:       {{home}}\bin\rg.exe
Argumentos: -i, {{query}}, {{documents}}
Nombre:     buscar en mis documentos
```

Wikipedia funciona igual, con la respuesta en la ruta de la URL en vez de en la cadena de consulta: `https://es.wikipedia.org/wiki/{{query}}`.

### La fecha y la hora

Cuatro variables las responde el reloj en lugar del equipo:

- `{{fecha}}` — hoy, día/mes/año (`25/08/2026`)
- `{{date}}` — hoy, mes/día/año (`08/25/2026`)
- `{{hora}}` — la hora actual, formato de 24 horas (`15:07`)
- `{{time}}` — la hora actual, formato de 12 horas (`3:07 PM`)

El nombre elige el formato, no el idioma en el que esté funcionando Launchtype. Una sustitución se escribe en el idioma en el que se va a *leer*, así que si escribes a un cliente de Estados Unidos querrás `{{date}}` en esa línea y `{{fecha}}` en el resto.

Nunca se preguntan. Si lo que quieres es que *te pregunte* una fecha —el día en que se solicitó un informe, por ejemplo— ponle otro nombre: `{{fecha de solicitud}}` es una pregunta, `{{fecha}}` es hoy.

### Tus propias variables

`{{hola}}`, `{{firma}}`, `{{aviso legal}}`: un nombre y el texto que escribes una y otra vez. Lo escribes una vez, lo usas en tantas sustituciones y comandos como quieras, y el día que cambie lo cambias en un solo sitio.

Se guardan en `snippets/placeholders.json`, un objeto JSON normal que puedes abrir y editar a mano:

```json
{
  "hola": "Hola, ¿qué tal?",
  "firma": "Un saludo,\nOscar"
}
```

Una variable puede estar escrita a partir de las demás, incluidas las del reloj y las preguntas:

```json
{
  "encabezado": "Madrid, {{fecha}}",
  "cierre": "{{encabezado}}\n{{firma}}"
}
```

Un nombre que acaba llegando a sí mismo —directamente o dando la vuelta por otros dos— se detiene ahí y se queda en pantalla como `{{nombre}}`, en lugar de dar vueltas.

Las de Launchtype mandan. Una variable tuya llamada `home`, `query` o `fecha` se rechaza al escribirla, porque esos nombres ya significan algo en todas partes.

Tienen un modo propio, `_` (guion bajo). Lista todo lo que hay en el archivo, y puedes buscar tanto por el nombre como por lo que dice; los botones de siempre hacen lo de siempre: **Añadir** escribe una nueva, **Editar** cambia la que tengas seleccionada y **Eliminar** la borra. Intro también edita: aquí no hay nada que lanzar, y una variable se usa *nombrándola* desde una sustitución o un comando, no desde esta lista.

### Cómo añadirlas

En los diálogos Añadir y Editar comando, los botones "Variable de ruta..." y "Variable de argumento..." abren un menú con todas las variables, su descripción y a qué se resuelven en este equipo. `{{query}}` encabeza el menú (es la única que no tiene nada que resolver) y tus propias variables van después de las de Launchtype, cada una con el texto al que equivale. Al elegir una se inserta donde está el cursor y el foco se queda en el campo, para que puedas seguir escribiendo.

El diálogo de sustituciones tiene el mismo menú en "Insertar variable...", con el reloj y las tuyas: una firma con `{{programfiles}}` dentro no se le ocurre a nadie, aunque sigue funcionando si la escribes. Y el diálogo Añadir/Editar variable también, que es la forma de escribir una variable a partir de las demás.

El botón Examinar también lo hace por ti: si eliges un archivo dentro de tu carpeta de usuario, se guarda como `{{home}}\...` en lugar de dejar tu nombre de usuario fijado.

### La comprobación al iniciar

Cuando Launchtype encuentra rutas que solo funcionan en este equipo, ofrece reemplazarlas una vez, al iniciar. La lista se agrupa por regla y no por comando (una sola línea que dice `C:\Program Files\Google\Chrome\Application\chrome.exe pasa a ser {{chrome}} (189 en uso)` en vez de 189 filas distintas) y todo viene marcado por omisión. "Corregir seleccionados" las reescribe, "Ahora no" vuelve a preguntar la próxima vez y "No preguntar nunca más" desactiva la comprobación (puedes reactivarla en Ajustes).

El mismo diálogo lista también las rutas que ninguna variable puede rescatar: una letra de unidad o un recurso de red que solo existe en el equipo donde se añadió el comando. Se muestran solo a título informativo; esos comandos hay que editarlos o borrarlos a mano. Nunca abren el diálogo por sí solas, así que unas cuantas unidades muertas no te darán la lata en cada arranque.

## Modo rutas

Pulsa `/` (barra) en el campo de entrada y Launchtype mira lo que has copiado. Archivos copiados en el Explorador o en el Finder, una ruta de "Copiar como ruta de acceso", varias rutas pegadas desde una terminal, una URL `file://` del navegador: todo llega igual. La lista de resultados pasa entonces a ser lo que se puede hacer con esos archivos, y cada fila dice sobre cuáles lo hará, así que lo que oyes antes de pulsar intro es exactamente lo que va a pasar: "Convertir recording.wav a FLAC", "Resumir 3 archivos con Claude", "Abrir una terminal en música".

Al entrar en el modo se anuncia lo que ha encontrado: el nombre si hay un archivo, cuántos si hay varios, y "no hay nada en el portapapeles" si no hay nada. Si copias algo nuevo, elige "Volver a leer el portapapeles" en lugar de salir y volver a entrar.

Solo se listan las filas que tienen sentido. Una carpeta de MP3 no ofrece "convertir a MP3"; un archivo de texto no ofrece ninguna conversión; y solo un vídeo ofrece "extraer la pista de audio".

### Convertir audio

El trabajo lo hace `ffmpeg`, así que tiene que estar instalado: en el `PATH`, en alguna de las carpetas de instalación habituales, o indicado en los ajustes. Los formatos de destino son MP3, FLAC, WAV, M4A y OGG, y un vídeo se convierte igual de bien que un archivo de sonido: apúntalo a un MKV y pide MP3 y obtienes la banda sonora.

**Cada conversión se comprueba antes de darla por buena.** `ffprobe` lee el archivo resultante, y solo cuenta como convertido si de verdad tiene audio y dura aproximadamente lo mismo que el original. ffmpeg termina correctamente con muchos archivos que solo ha escrito a medias, y esta es la diferencia entre darse cuenta y no darse cuenta.

Cuando una conversión pasa esa comprobación se borra el original, que es el sentido del modo: lo has convertido porque querías el otro formato. Desmarca "Borrar el archivo original tras una conversión verificada" en los ajustes para quedarte con los dos. Una conversión que no pasa la comprobación borra *el resultado* y deja tu archivo exactamente donde estaba; se te dice qué archivo y por qué.

Nunca se sobrescribe nada. Si `song.flac` ya existe, obtienes `song (2).flac`.

**"Extraer la pista de audio"** es otra cosa distinta de convertir, y solo aparece con vídeos: el audio se copia fuera del contenedor sin recodificarlo, así que no se pierde nada y una película de dos horas tarda un segundo. Aterriza en el contenedor que le corresponde a ese códec — AAC en `.m4a`, Vorbis en `.ogg` — y el vídeo se queda como estaba. Un códec que no tiene contenedor propio lo dice en lugar de recodificar por su cuenta.

### Transcribir

**Claude no puede escuchar audio.** La API acepta texto, imágenes y PDF y nada más, así que la transcripción la hace en tu propio equipo Whisper, que se instala aparte: el `whisper-cli` de whisper.cpp, el comando `whisper` de OpenAI, `whisper-ctranslate2` o cualquiera equivalente. Launchtype lo busca en el `PATH`, o lo indicas en los ajustes junto con el modelo a usar: un nombre como `base` o `small` para los comandos estilo OpenAI, la ruta de un archivo `ggml-*.bin` para whisper.cpp.

Sea cual sea la grabación, ffmpeg la descodifica primero, así que se puede transcribir cualquier formato que ffmpeg lea. La transcripción va al portapapeles *y* se guarda como un `.txt` junto a la grabación, de modo que una transcripción larga no haya que repetirla nunca; y el modo pasa entonces a listar esa transcripción, lista para resumirla o corregirla.

### Preguntar a Claude

Esto usa la suscripción de Claude Code con la que ya has iniciado sesión, la misma que usan las descripciones de capturas, con el modelo que hayas elegido en los ajustes. Funciona con archivos de texto y PDF; si lo apuntas a una grabación, primero se transcribe y lo que lee Claude es la transcripción.

- **Resumir**: qué son los archivos y qué contienen, leído en voz alta y copiado.
- **Preguntar a Claude sobre...**: escribes una pregunta y Claude la responde a partir de los archivos. Se lee en voz alta y se copia.
- **Corregir**: ortografía, gramática y puntuación, con el texto corregido copiado y listo para pegar. Solo archivos de texto: te devuelve lo que escribiste, corregido.
- **Traducir...**: escribes un idioma y se copia la traducción conservando la disposición del original.

Nunca se recorta nada en silencio. Si hay demasiado texto para enviarlo de una vez, o el PDF es demasiado grande, se dice exactamente eso: un resumen de la primera quinta parte de un documento, presentado como el resumen del documento, sería peor que no tener resumen.

### Lo demás

- **Información multimedia** lee cuánto dura un archivo, su códec, frecuencia de muestreo, canales, tasa de bits, tamaño y el tamaño del vídeo si lo tiene, y copia esa misma línea para que puedas pegarla.
- **Información de texto** es lo mismo para un archivo de texto: líneas, palabras, caracteres y kilobytes.
- **Copiar el contenido** pone un archivo de texto en el portapapeles; si son varios, se unen bajo sus nombres.
- **Abrir en Visual Studio Code** abre todo en una sola ventana.
- **Abrir una terminal** abre una carpeta: si es un archivo, la carpeta donde está, y la fila dice de qué carpeta se trata. Las repetidas se agrupan, así que cinco archivos de la misma carpeta abren una sola terminal.

Las conversiones y las transcripciones se hacen en segundo plano y la ventana sigue siendo utilizable mientras tanto; un segundo intro mientras hay algo en marcha se rechaza en lugar de empezar el mismo trabajo dos veces.

## Ajustes

El botón Ajustes de la interfaz abre un diálogo donde puedes guardar estas preferencias en `settings.json`:

- Habilitar sonidos
- Arrancar minimizado
- Arrancar en modo sustituciones al invocar
- Buscar rutas específicas de este equipo al iniciar (ver [Rutas portables](#rutas-portables-variables))
- Ruta de la biblioteca de Steam
- Modelo de IA para las descripciones de capturas (Claude Opus, Sonnet o Haiku)
- Idioma de la interfaz (el mismo del sistema, inglés o español); se aplica al reiniciar la aplicación
- Archivo de comandos: una lista desplegable con todos los `.json` con forma de comandos que hay junto a la aplicación, para mantener conjuntos separados (trabajo, casa, un juego) y cambiar entre ellos sin reiniciar. También puedes escribir un nombre nuevo para empezar uno desde cero.
- Servidor, puerto, usuario, archivo de clave privada y contraseña de SSH, que usa el modo SSH
- Minutos sin usar tras los que se bloquea la caja fuerte, y segundos que un secreto copiado puede quedarse en el portapapeles (ver [Caja fuerte cifrada](#caja-fuerte-cifrada))
- Programa o carpeta de ffmpeg, si una conversión verificada borra el original, y el transcriptor Whisper y su modelo, todo ello para el [modo rutas](#modo-rutas). Deja vacías las dos casillas de programa para que se busquen en el `PATH`.

Los parámetros de línea de comandos tienen prioridad sobre estos ajustes durante la ejecución actual (por ejemplo, pasando `-q` se desactivan los sonidos aunque el ajuste esté habilitado, y pasando `-m` se arranca minimizado aunque el ajuste esté desactivado).

## Importar otro archivo de comandos

El botón "Importar..." trae comandos desde un segundo `commands.json`: el de tu otro equipo, un conjunto que te ha pasado alguien, o el archivo del trabajo que tienes junto al de casa. Búscalo y, si de verdad es un archivo de comandos, un diálogo lista lo que contiene.

**Solo se listan los comandos que aún no tienes.** Un comando cuenta como ya presente cuando comparte el id con uno de los tuyos, o cuando apunta a la misma ruta con los mismos argumentos y el mismo nombre para mostrar (las mayúsculas y `\` frente a `/` no cuentan como diferencia). El diálogo dice cuántos se han dejado fuera por ese motivo.

Importar solo añade, y en eso consiste todo el diseño:

- nada de lo que ya está en tu lista se edita, se reapunta, se renombra ni se elimina;
- nunca se te pregunta si quieres reemplazar un comando que ya tienes: esos ni siquiera aparecen en la lista;
- lo peor que puede hacer una importación descuidada es añadir filas, que luego puedes eliminar.

Todo aparece marcado de entrada; "Seleccionar todo" y "Seleccionar ninguno" están ahí para cuando solo quieres unos pocos de un archivo largo. Cada fila se lee como el nombre del comando, su ruta y lo que hayan detectado las comprobaciones:

- **su atajo ya está ocupado**, por uno de tus comandos o por un comando anterior del mismo archivo. Solo se ejecuta el primer comando que coincide con un atajo, así que la copia importada entra sin él en lugar de llegar muerta; ponle un atajo nuevo desde el diálogo de edición.
- **una variable sin resolver**: un `{{error}}`, o una variable de una versión más reciente, en la ruta o en los argumentos. Se lanzaría literalmente.
- **una ruta que aquí no existe**: un programa que aquel equipo tenía y este no. Es solo un aviso: impórtalo igualmente si estás a punto de instalarlo.

Nada de esto impide importar; está ahí para que nada llegue por sorpresa. Los comandos importados empiezan con cero usos, porque esos usos ocurrieron en el otro equipo.

## Sustituciones

Las sustituciones son fragmentos de texto que, al escribir su nombre de archivo en la caja de texto, se copian al portapapeles.

Para usarlas hay que crear archivos .txt dentro de la carpeta snippets de la aplicación. En modo sustituciones el botón Añadir te crea una, Editar cambia la que tengas seleccionada y Eliminar la borra; "Abrir carpeta de sustituciones" abre esa carpeta en el explorador de archivos.

El nombre del archivo es el acceso abreviado (sin la extensión .txt) y el contenido es lo que se copia.

Por ejemplo, con un archivo email.txt que contenga mi_email@gmail.com, basta con escribir "email" en la caja y pulsar Intro para tener tu email en el portapapeles.

Para acceder a las sustituciones debes estar en modo sustituciones: escribe un guion (-) en la caja. Desaparecerán los comandos y aparecerán las sustituciones.

Para volver a comandos, escribe un punto (.). En cualquier caso, cada vez que se invoca con el atajo del lanzador la aplicación arranca en modo comandos, así que no hace falta hacer nada.

### Sustituciones que preguntan

Una sustitución se copia tal cual, y eso cubre una firma. Lo que no cubre es el correo que mandas dos veces por semana cambiando dos palabras: una sustitución por informe, si el texto tiene que ir literal.

Escribe esas dos palabras como `{{un nombre}}` y la sustitución pasa a ser una plantilla. Al pulsar Intro, en lugar de copiarse, la ventana se queda abierta y te va pidiendo cada una, igual que hace un comando con su `{{query}}`: los mismos sonidos, la misma numeración, Escape para salirte, y el texto ya relleno en el portapapeles en cuanto entra la última respuesta.

```
snippets/informe.txt:

Hola, te envío el informe de {{informe}}, solicitado el {{fecha de solicitud}}.
Un saludo.
```

Eso hace dos preguntas —"informe, parámetro de consulta 1", "fecha de solicitud, parámetro de consulta 2"— y el nombre se dice primero, porque el nombre es lo que te indica qué escribir.

Una sola regla decide qué se pregunta y qué no: **un nombre que Launchtype conoce se rellena, y uno que no conoce se pregunta.** Lo que conoce es [todo lo de arriba](#rutas-portables-variables): las carpetas y navegadores de este equipo, el `{{date}}`/`{{fecha}}`/`{{time}}`/`{{hora}}` del reloj, y tus propias variables. Así que `{{fecha}}` es hoy sin preguntar, `{{firma}}` es tu firma sin preguntar, y `{{informe}}` —que no es el nombre de nada— es una pregunta.

Un nombre se pregunta **una vez** por muchas veces que aparezca, y se rellena en todas. Para eso sirve ponerle nombre: `{{nombre}}` tres veces en una carta es una cosa dicha tres veces, no tres cosas que escribir. Ni las mayúsculas ni los espacios importan: `{{Informe}}`, `{{ informe }}` e `{{INFORME}}` son una sola pregunta, y se te pide con la primera forma que hayas escrito.

`{{query}}` es la excepción, y conserva el significado que tiene en un comando: una pregunta por cada uno, en el orden en que aparecen, escribas los que escribas.

Las preguntas se buscan también dentro de tus propias variables, así que una variable que lleve `{{nombre}}` hace que todas las sustituciones que la usan pregunten por el nombre.

Nada se codifica ni se reescribe camino del portapapeles: una sustitución es texto, y llega tal como lo escribiste.

Los diálogos Añadir y Editar sustitución cuentan todo esto en una línea encima de la caja de contenidos, y tienen un botón "Insertar variable..." con el reloj, tus variables y la forma de escribir una nueva.

## Historial del portapapeles

El historial del portapapeles se abre escribiendo ? (signo de interrogación) en la caja. Muestra hasta 50 elementos de texto que hayas copiado y se conserva entre reinicios.

Solo funciona con elementos de texto, no con rutas de archivos u otros formatos.

## Lanzador de juegos de Steam

El modo de juegos de Steam se abre escribiendo , (coma) en la caja. Este modo escanea tu biblioteca de Steam en busca de juegos instalados y te permite lanzarlos directamente.

El escáner busca los juegos instalados en la carpeta de la biblioteca de Steam (por defecto: C:\Program Files (x86)\Steam\steamapps) analizando los archivos appmanifest. Puedes indicar una ruta personalizada con el parámetro `-l` o desde el diálogo de Ajustes.

Estando en modo Steam, puedes buscar juegos por nombre con búsqueda difusa igual que con los comandos. Al seleccionar un juego se lanza a través de Steam.

Para volver a comandos, pulsa la tecla punto (.).

## Aplicaciones

El modo aplicaciones se abre escribiendo @ (arroba) en la caja. Lista todos los programas instalados en este equipo — sin añadir nada, sin configurar nada — y lanzar uno funciona igual que lanzar un comando: escribe lo suficiente de su nombre para que aparezca, llega a él con las flechas y pulsa Intro.

De dónde sale la lista depende de la plataforma:

- **Windows**: la carpeta Aplicaciones del shell, la misma carpeta virtual que abre `shell:AppsFolder` y en la que busca el menú Inicio. Eso incluye los programas de escritorio (todo lo que tenga entrada en el menú Inicio), las aplicaciones de la Microsoft Store y otras empaquetadas, y las entradas del panel de control que Windows genera — Administrador de tareas, Administración de impresión, los símbolos del sistema de Visual Studio. Cada una se lanza a través del mismo shell que la listó, así que una aplicación de la Store arranca igual que desde el menú Inicio, y un programa lanzado así nunca se eleva solo porque Launchtype lo esté.
- **macOS**: todos los paquetes de aplicación del índice de Spotlight, más un recorrido de `/Applications`, `/System/Applications` y `~/Applications` (un nivel dentro de sus subcarpetas) para equipos con la indexación desactivada. Los paquetes se lanzan con `open`, que es la única forma admitida de arrancar uno.

No se filtra nada por parecer poco interesante. Si el menú Inicio o el Launchpad llegan a algo, `@` también llega, incluidos archivos de ayuda y desinstaladores.

Los juegos de Steam son la única excepción: ya tienen su propio modo `,`. Windows coloca un acceso directo en el menú Inicio junto a cada juego instalado — 78 de 433 filas en el equipo donde se escribió esto — así que sin esto `@` leería la biblioteca entera por segunda vez. Todo lo que se lance con una URL `steam://` se deja a `,`; Steam en sí es un programa como cualquier otro y se queda.

La búsqueda ignora los acentos tanto de lo que escribes como de lo que hay en la lista, porque estos nombres llegan en el idioma en el que corre el sistema: en un Windows en español, `administracion` encuentra "Administración de equipos" sin tocar las teclas de acentos. La lista se ordena igual, de modo que los nombres acentuados quedan junto a sus vecinos y no detrás de la "z".

El escaneo se ejecuta cada vez que entras al modo — un programa instalado esta mañana está ahí sin reiniciar Launchtype — y tarda unos cientos de milisegundos, que transcurren mientras aún se está leyendo el anuncio del modo.

Aquí el botón Copiar Argumentos pasa a llamarse "Copiar archivo del programa (Alt+O)" y copia la ruta al ejecutable de la aplicación seleccionada — útil para convertir en comando guardado algo que has encontrado con `@`. En Windows la ruta es aquella a la que la carpeta Aplicaciones dice que apunta la entrada, así que funciona incluso con programas cuya identidad no revela nada: Firefox aparece con el id opaco `308046B0AF4A39CB` y aun así da `C:\Program Files\Mozilla Firefox\firefox.exe`. Las aplicaciones de la Store y demás empaquetadas no tienen esa ruta — Windows las arranca por identidad, y sus archivos viven en una carpeta `WindowsApps` protegida con un nombre que cambia en cada actualización — así que en esos casos se dice "Esta aplicación no tiene ningún archivo de programa que copiar". En macOS la ruta es el paquete `.app`, que es lo que un Mac entiende por la aplicación.

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

**Recortar una región concreta** pregunta qué buscar en un diálogo con un único campo — escribe algo como `el botón aceptar` y pulsa Aceptar, o pulsa Cancelar para dejarlo. Si la IA lo encuentra, el recorte acaba en el portapapeles; si no, te dice por qué.

Las funciones de IA usan **tu sesión existente de Claude o de ChatGPT**, no una clave de API: primero el token OAuth de Claude Code de `~/.claude/.credentials.json`, y como alternativa el token de la CLI de Codex en `~/.codex/auth.json`. Si no hay ninguno, la aplicación te lo dice. El modelo que se usa con Claude se elige en el diálogo de Ajustes.

Aquí nada falla en silencio. Cuando algo sale mal en la captura, al guardarla o en la llamada a la IA — no hay sesión iniciada, ha caducado, no hay red, el disco está lleno, la IA no encuentra el elemento — el motivo se lee en voz alta *y* se muestra en un diálogo de error, que vuelve a sacar la ventana para que el mensaje no pase desapercibido.

Para volver a comandos, pulsa la tecla punto (.).

## Temporizadores

El modo de temporizadores se abre escribiendo `[` (corchete izquierdo) en la caja. Los temporizadores cuentan atrás durante unos minutos y luego te avisan.

Añade uno con el botón Añadir, o modifícalo con el botón Editar. El diálogo permite configurar:

- Un **título** y una **descripción** (que se anuncian por el lector de pantalla al dispararse).
- El número de **minutos** de la cuenta atrás.
- Una casilla de **repetición**.
- Un **sonido**, elegido entre los tonos incluidos en `sounds/timers/`. Selecciona «Archivo personalizado...» para usar cualquier .wav de tu sistema con Examinar, o «Sin sonido» para el pitido del sistema. Cada opción suena al llegar a ella, así que puedes recorrer la lista con las flechas para escuchar los tonos.

Cuando salta un temporizador se anuncian su título y su descripción, y su sonido —o el pitido— se repite hasta que pulsas Ctrl+Alt+Espacio (Ctrl+Cmd+Espacio en Mac). Uno que salte mientras estás lejos del teclado sigue sonando cuando vuelves, y el atajo lo calla aunque haya un diálogo abierto.

Editar un temporizador que está contando reinicia la cuenta atrás con los nuevos minutos.

Los temporizadores aparecen en la lista con su estado actual:

- Los **no repetitivos** aparecen como `parado` hasta que se inician. Al ejecutarlos (Intro o Alt+R) empieza la cuenta atrás; ejecutarlos de nuevo mientras cuentan **reinicia** el temporizador. Se disparan una vez y se detienen.
- Los **repetitivos** se disparan cada X minutos hasta que se desactivan. Vienen **activados** por defecto, y ejecutarlos (Intro o Alt+R) los **alterna** entre activado y desactivado.

Para volver a comandos, pulsa la tecla punto (.).

## Alarmas

El modo de alarmas se abre escribiendo `]` (corchete derecho) en la caja. Las alarmas se disparan una vez al día a una hora concreta en formato de 24 horas.

Añade una con el botón Añadir, o modifícala con el botón Editar. El diálogo permite configurar:

- Un **título** y una **descripción** (que se anuncian por el lector de pantalla al dispararse).
- La **hora** (0-23) y los **minutos** (0-59).
- Un **sonido**, elegido entre los tonos incluidos en `sounds/alarms/`. Selecciona «Archivo personalizado...» para usar cualquier .wav de tu sistema con Examinar, o «Sin sonido» para el pitido del sistema. Cada opción suena al llegar a ella, así que puedes recorrer la lista con las flechas para escuchar los tonos.

Igual que con los temporizadores, al saltar una alarma se anuncian su título y su descripción, y su sonido —o el pitido— se repite hasta que pulsas Ctrl+Alt+Espacio (Ctrl+Cmd+Espacio en Mac).

Editar una alarma mantiene su estado de activada o desactivada.

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

## Modo SSH

Escribe `$` en el campo de entrada para abrir una consola remota en el servidor configurado en Ajustes. Launchtype se conecta una vez y mantiene la conexión abierta, así que solo el primer comando paga el coste del saludo inicial.

La autenticación prefiere el archivo de clave privada cuando hay uno configurado, en cualquier formato que acepte OpenSSH, incluidos `-----BEGIN OPENSSH PRIVATE KEY-----` y las claves RSA en PEM antiguas. Si la clave está protegida con una frase de contraseña, se prueba el campo de contraseña como esa frase. Sin archivo de clave, la contraseña se usa para autenticación por contraseña.

Escribe un comando y pulsa intro. La salida estándar vuelve como un elemento de lista por línea, que puedes recorrer con las flechas; pulsar intro sobre una línea con el campo de entrada vacío copia esa línea al portapapeles. Todo lo que el comando haya escrito en la salida de errores se muestra en un aviso con un botón Aceptar.

Todos los comandos se ejecutan en una única consola de inicio de sesión persistente, así que `cd`, las variables exportadas y todo lo que configuren tus scripts de inicio se conserva entre comandos. Al conectar, Launchtype también carga `~/.zshrc` o `~/.bashrc` (con la expansión de alias activada), de modo que los alias y las rutas añadidas al PATH de tu configuración interactiva también funcionan; todo lo que esos scripts escriban en la salida de errores se muestra una vez en un aviso. Una excepción: un `.bashrc` que empiece con una comprobación de interactividad (`case $- in *i*) ;; *) return;; esac`, habitual en Debian y Ubuntu) se sigue saltando a sí mismo; coloca los alias que quieras usar en Launchtype antes de esa comprobación. Sigue sin haber un terminal: los programas que piden datos por teclado o dibujan pantallas completas (vim, top) no funcionarán.

## Estadísticas de uso

El modo de estadísticas se abre escribiendo `!` (signo de exclamación) en la caja. Es una lista de solo lectura que muestra cuántos comandos has ejecutado en total, tus 10 comandos más usados y los 10 menos usados.

Para volver a comandos, pulsa la tecla punto (.).

## Modo emojis

Pulsa `:` (dos puntos) en la caja y escribe lo que *es* el emoji. Los nombres son los mismos que dice tu lector de pantalla, así que "cara sonriendo" encuentra 😀 y "corazón rojo" encuentra ❤️.

No hace falta saberse el nombre oficial. Cada emoji lleva además sus palabras clave de CLDR, así que "risa" llega a "cara llorando de risa" y "café" llega a "bebida caliente". Escribe un par de palabras y la mejor coincidencia queda la primera; pulsa intro para copiar el emoji al portapapeles y pégalo donde estuvieras.

Los nombres y las palabras clave siguen el idioma de la aplicación, y las tildes son opcionales: "corazon" funciona igual de bien que "corazón". La lista muestra solo el nombre, nunca el emoji al lado, porque si no el lector de pantalla leería lo mismo dos veces en cada flecha.

Toda la tabla va compilada dentro del programa: no se descarga nada, y Windows y macOS dan resultados idénticos. Hay casi dos mil emojis y la lista enseña las 200 mejores coincidencias a la vez, así que añade una palabra si lo que buscas todavía no aparece. Las variantes de tono de piel no se incluyen.

## Modo conversión de unidades

Pulsa `=` (igual) en la caja y escribe un número. La lista se convierte en ese número convertido de todas las formas posibles: `100` da "100 grados Celsius = 212 grados Fahrenheit", luego "100 pies = 3048 centímetros", "100 kilogramos = 220.4623 libras" y así hacia abajo, con las conversiones de todos los días primero. Pulsa intro en la que quieras y el número solo se copia al portapapeles, listo para pegar. La ventana se queda abierta, porque después de una conversión suele venir otra.

El número no es una búsqueda: es lo que se convierte. Lo que buscas son las palabras que escribas después. `100 pies cm` va directo a "100 pies = 3048 centímetros", y `70 kg libras` a "70 kilogramos = 154.3236 libras". La primera unidad que nombras es desde la que conviertes, así que `100 cm pies` es el sentido contrario. Las palabras de enlace son opcionales y se entienden en ambos sentidos: `100 cm a pulgadas` funciona igual que `100 cm pulgadas`. Si nombras una sola unidad, como en `2 kg`, la lista primero la convierte a todo lo demás y después convierte todo lo demás a ella.

Las unidades responden tanto a su símbolo como a su nombre, y cada fila te dice cuál es ese símbolo poniéndolo entre paréntesis: "100 centímetros (cm) = 39.3701 pulgadas (in)", "10 millas por hora (mph) = 16.0934 kilómetros por hora (km/h)". Así te enteras de qué escribir recorriendo la lista y no leyendo esta página. Un símbolo con barra funciona de las dos formas, `km/h` o `kmh`. Los paréntesis se omiten cuando el nombre ya lo dice —"1 psi", "42 talla de calzado europea de hombre (eu)" sí lo lleva porque el nombre no dice "eu"—, porque si no se leería dos veces.

Las palabras largas también coinciden por dentro, así que `galón` llega al estadounidense y al imperial; en cambio un símbolo de una o dos letras solo coincide donde empieza una palabra, así que escribir `l` encuentra litros y libras (por "lb") y no arrastra todas las calorías y kelvin que llevan una l en medio.

La coma es un separador decimal (`1,5 kg libras`), el número puede ser negativo (`-40 c f`, la única temperatura en la que las dos escalas coinciden) y el espacio tras el número es opcional (`100pies cm`).

Lo que hay dentro: longitud, masa, temperatura, volumen (métrico, estadounidense e imperial, hasta cucharaditas y tazas), superficie, velocidad, presión, energía, potencia, datos —tanto el gigabyte con el que se vende un disco como el gibibyte con el que lo cuenta el sistema operativo—, tiempo, ángulos, par motor y consumo de combustible, el que va al revés: más millas por galón son menos litros a los 100 km. El mes es el mes gregoriano medio y el año son 365.2425 días, así que "cuántas horas tiene un mes" también tiene respuesta.

También están las tallas de calzado, de hombre y de mujer, entre Europa, Reino Unido, Estados Unidos, Japón, China, Corea, México, Brasil, Rusia, Australia y la India. `42 eu us zapato hombre` lee la fila de la tabla y da "42 talla de calzado europea de hombre = 8.5 talla de calzado estadounidense de hombre"; las tallas que caen entre dos filas de la tabla se interpolan, así que las medias tallas también tienen respuesta. Salen de tablas de equivalencias publicadas y son tan aproximadas como esas tablas: hay países que venden de verdad con las tallas de otro (Australia y la India con las británicas, China y Corea en milímetros Mondopoint), y por eso esas filas coinciden.

Cada conversión es una fórmula compilada dentro del programa, así que el modo no necesita red y da siempre la misma respuesta. Por eso mismo no hay monedas: un tipo de cambio son noticias y no aritmética, y el modo `+` ya trae las que merecen la pena.

Los nombres y las palabras de búsqueda siguen el idioma de la aplicación, así que también puedes escribir `100 ft cm` o `70 kg lb`. La lista enseña las 300 mejores filas a la vez —con solo un número escrito, las conversiones de todos los días primero—, así que añade una palabra si lo que buscas todavía no aparece.

## Caja fuerte cifrada

Pulsa `*` (asterisco) en el campo de entrada para abrir la caja fuerte: contraseñas, códigos de recuperación, claves de licencia y cualquier otra cosa que no debería estar en una sustitución. Funciona igual que el modo sustituciones —escribes, te mueves a la entrada, pulsas Intro y ya está en el portapapeles—, salvo que nada se puede leer hasta que hayas dado la contraseña maestra, y nada se escribe nunca en el disco sin cifrar.

La primera vez que pulses `*` se te pedirá que elijas una contraseña maestra. Esa contraseña no se guarda en ningún sitio, no se puede recuperar y no se puede restablecer: si la pierdes, pierdes la caja fuerte con ella. Todo vive en una carpeta `vault` junto a la aplicación, al lado de `snippets` y las demás, así que viaja con una instalación portable; eso sí, haz la copia de seguridad de la carpeta entera, porque `vault.meta` y las entradas no sirven de nada por separado.

Añadir, Editar y Eliminar funcionan como en el resto de la aplicación. Cada entrada tiene un nombre, un acceso abreviado opcional (una coincidencia exacta te lleva directamente a ella, como en los otros modos) y el secreto en sí, que puede ocupar varias líneas para un bloque de códigos de recuperación o una clave. Eliminar pregunta antes, porque no hay deshacer ni una segunda copia.

Una vez abierta, la caja fuerte se vuelve a bloquear sola tras cinco minutos sin usarla, y la clave se borra de la memoria —no simplemente se ignora— gracias a un temporizador en segundo plano, así que alejarte del ordenador la cierra. Pon el tiempo a 0 en los Ajustes y te pedirá la contraseña maestra en cada copia. "Bloquear la caja fuerte ahora" y "Cambiar la contraseña maestra" están al final de la lista; cambiar la contraseña es instantáneo, porque las entradas no están cifradas con la contraseña en sí (ver más abajo).

### Copiar, y el portapapeles

Darte una contraseña significa ponerla en el portapapeles, que es un sitio realmente expuesto donde dejarla. Alrededor de cada copia pasan dos cosas:

- **Nunca llega al historial del portapapeles.** Se le dice al vigilante del portapapeles que rechace ese valor exacto *antes* de escribirlo, así que el modo `?` nunca lo lista y nunca acaba en `clipboard_history.json`. Sin esto, cada contraseña que consultaras terminaría en un JSON en texto plano junto a la aplicación.
- **Se retira del portapapeles.** Treinta segundos después (configurable en los Ajustes, 0 para desactivarlo) se vacía el portapapeles, pero solo si el secreto sigue siendo lo que hay en él, así que lo que hayas copiado mientras tanto no se toca.

### Cómo está cifrada

Merece la pena decirlo claramente, porque "cifrado" por sí solo significa muy poco:

- La contraseña maestra se estira con **Argon2id** a 256 MiB y cuatro pasadas: alrededor de medio segundo por desbloqueo en un equipo normal, y la razón por la que adivinarla sale caro con el hardware que alguien traería para ello. Cada caja fuerte guarda el coste con el que se creó, así que subirlo más adelante no deja inservible una caja fuerte ya existente.
- Eso da una clave maestra que no hace más que desenvolver una **clave de la caja fuerte** aleatoria de 32 bytes; las entradas se sellan con esa. Por eso cambiar la contraseña maestra reescribe un archivo pequeño en vez de volver a cifrarlo todo.
- Cada entrada es un archivo aparte sellado con **AES-256-GCM** bajo un nonce aleatorio nuevo, así que manipular uno se detecta en vez de darlo por bueno.
- **Los nombres de las entradas también están dentro del cifrado.** Los archivos se llaman como un uuid aleatorio, no como la entrada, porque un listado de carpeta lleno de `amazon.enc` y `vpn-trabajo.enc` revela casi todo lo que vale una lista de contraseñas. Quien consiga la carpeta sabrá cuántas entradas hay y más o menos cuánto ocupa cada una; nada más. El uuid se autentica junto con el contenido, así que tampoco se pueden intercambiar los archivos entre sí.
- El texto descifrado solo existe en memoria, en búferes que se borran solos al liberarse, y solo se descifra un secreto cada vez: al desbloquear se leen los nombres, no los secretos.

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
| `@` | Aplicaciones | Lanzar cualquier programa instalado en este equipo |
| `'` | Capturas | Capturar, describir o recortar una ventana o la pantalla completa |
| `[` | Temporizadores | Cuenta atrás de X minutos (una vez o repetitiva) |
| `]` | Alarmas | Se disparan a una hora del día (formato 24 horas) |
| `#` | Notebrook | Publicar una nota rápida en tu Notebrook |
| `+` | Datos en tiempo real | Leer en voz alta precios, tiempo, titulares y temperaturas del ordenador |
| `!` | Estadísticas | Comandos más y menos usados |
| `$` | SSH | Ejecuta comandos en un servidor remoto y lee la salida |
| `:` | Emojis | Buscar un emoji por su descripción y copiarlo |
| `=` | Conversión de unidades | Convertir un número entre unidades, tallas de calzado incluidas |
| `*` | Caja fuerte cifrada | Contraseñas y otros secretos, cifrados tras una contraseña maestra |
| `_` | Variables de sustitución | Las variables `{{nombre}}` que escribes tú, y que usan tanto las sustituciones como los comandos |
| `/` | Rutas | Actúa sobre los archivos del portapapeles: convertir, transcribir, preguntar a Claude, abrir |
| `.` | (cualquier modo) | Volver al modo Comandos |

## Retroalimentación de audio

La aplicación emite sonidos ante distintas acciones:

- Sonido de arranque al iniciar la aplicación
- Sonidos de mostrar/ocultar al alternar la ventana
- Sonido de coincidencia cuando se encuentra un acceso abreviado exacto
- Sonido al escribir cuando cambian los resultados de búsqueda
- Sonido de pregunta cuando un comando o una sustitución empieza a pedir un parámetro, junto con el "Parámetro de consulta 1" hablado
- Sonido de escritura de consulta en cada pulsación mientras respondes, en lugar de los sonidos de coincidencia y de escritura: el campo de entrada está recogiendo una respuesta, no buscando
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
