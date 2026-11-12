# Registro de cambios

[简体中文](zh-CN.md) · [繁體中文](zh-TW.md) · [English](en-US.md) · **Español** · [Deutsch](de-DE.md) · [日本語](ja-JP.md) · [Français](fr-FR.md) · [Русский](ru-RU.md)

Cambios visibles para el usuario en cada versión de Argus. La numeración sigue el [versionado semántico](https://semver.org/lang/es/). Este archivo es una traducción del [registro de cambios en chino simplificado](zh-CN.md), con correspondencia entrada por entrada.

## [0.5.0](https://github.com/Ray-lea/Argus/compare/v0.4.0...v0.5.0) - 2026-11-12

v0.5.0 añade la suspensión y la reanudación de procesos, la agrupación de procesos por usuario, cuatro columnas de información de procesos y anchos de columna calculados según el contenido. La lista de dispositivos de la página Rendimiento permite ocultar y reordenar los dispositivos. La interfaz se ofrece ahora también en chino tradicional, español, alemán, japonés, francés y ruso. Esta versión también evita que algunos sistemas con Intel RST se bloqueen tras iniciar Argus, corrige problemas como la imposibilidad de expulsar de forma segura los discos USB y reduce el uso de memoria y CPU con la ventana oculta.

### Nuevas funciones

- **Suspender y reanudar procesos**: Los menús de procesos y grupos de aplicaciones añaden «Suspender proceso» y «Reanudar proceso». Argus impide suspender los procesos críticos del sistema, el Administrador de ventanas del escritorio y el propio Argus, y solicita confirmación antes de suspender los demás. Los procesos suspendidos muestran «Suspendido» después del nombre. ([@digital-bombyx](https://github.com/digital-bombyx) [#65](https://github.com/Ray-lea/Argus/pull/65))
- **Columnas de información de procesos**: Se añaden cuatro columnas opcionales: «Versión del archivo», «Arquitectura», «Elevado» y «Línea de comandos». Están ocultas de forma predeterminada. Se pueden activar en el menú contextual de los encabezados de columna o en «Seleccionar columnas». ([@digital-bombyx](https://github.com/digital-bombyx) [#72](https://github.com/Ray-lea/Argus/pull/72))
- **Ancho de las columnas de procesos**: El ancho automático se ajusta al contenido y deja el espacio sobrante a la derecha. Arrastre el borde de una columna para ajustar su ancho o haga doble clic para restaurar el ancho automático. La nueva opción «Ancho completo» asigna el ancho sobrante a la columna «Nombre».
- **Agrupar procesos por usuario**: «Configuración → Idioma y apariencia → Agrupar procesos por usuario» ofrece tres modos: «Automático», «Agrupar siempre» y «No agrupar nunca». El modo automático solo agrupa los procesos cuando hay varios usuarios con sesión iniciada. El menú contextual de las filas de grupo ofrece «Administrar cuentas de usuario» y «Cerrar sesión».
- **Ocultar procesos del sistema**: Las páginas Procesos y Conexiones añaden «Ocultar procesos del sistema», desactivado de forma predeterminada y con un estado guardado por separado para cada página. Las listas «Mostrar siempre» y «También ocultar» se editan en Configuración. ([#32](https://github.com/Ray-lea/Argus/issues/32))
- **Árbol de procesos y resumen de recursos**: El árbol de procesos aparece contraído y el menú de los encabezados ofrece «Expandir todo» y «Contraer todo». «Resumen de recursos» admite «Automático», «Mostrar siempre» y «Oculto». El modo automático oculta el resumen cuando falta altura en la ventana.
- **Lista de dispositivos de la página Rendimiento**: Haga clic derecho en un dispositivo para ocultarlo o seleccione cada dispositivo en «Dispositivos mostrados». Arrastre un dispositivo para cambiar el orden. Los discos admiten orden por número, por letra de unidad o personalizado.
- **Vista multinúcleo de la CPU**: Cada gráfico de procesador lógico muestra el tiempo del núcleo, que se puede ocultar desde el menú del gráfico de CPU. Argus identifica los núcleos de eficiencia de bajo consumo de Intel (núcleos LP E) con un color y un filtro propios. Los gráficos se distribuyen preferentemente en filas y columnas sin huecos.
- **Información de hardware**: La página de la GPU muestra la versión del controlador, su fecha y la versión de DirectX, incluido el nivel de características. Los detalles del disco admiten la vida útil restante y el total de datos leídos y escritos de más SSD SATA. Las estadísticas de los HDD incluyen horas de encendido, ciclos de arranque y parada, sectores reasignados y sectores pendientes.
- **Idiomas de la interfaz**: Se añaden chino tradicional, español, alemán, japonés, francés y ruso. La interfaz, la bandeja, los mensajes del sistema y el instalador están traducidos. Los números y las fechas usan el formato regional de Windows. ([@abc0922001](https://github.com/abc0922001) [#45](https://github.com/Ray-lea/Argus/pull/45), [@Rimuwu](https://github.com/Rimuwu) [#51](https://github.com/Ray-lea/Argus/pull/51), [@s0ftw4r8](https://github.com/s0ftw4r8) [#52](https://github.com/Ray-lea/Argus/pull/52), [@khativari](https://github.com/khativari) [#55](https://github.com/Ray-lea/Argus/pull/55), [@hayxe1](https://github.com/hayxe1) [#62](https://github.com/Ray-lea/Argus/pull/62))
- **Novedades**: La página «Acerca de» y el diálogo de actualización muestran las novedades en el idioma de la interfaz. En «Configuración → Actualizaciones», «Ver novedades» abre el diálogo con las opciones «Expandir todo» y «Contraer todo». Las transiciones al abrir, expandir y pasar el puntero siguen «Animaciones de la interfaz».
- **Iconos y animaciones**: Los iconos de la interfaz proceden ahora de la biblioteca Lucide. Cuando el puntero pasa sobre un botón o un elemento de navegación, su icono reproduce una animación breve una sola vez. La animación no se reproduce si la opción «Animaciones de la interfaz» está desactivada.
- **Ventana y bandeja**: Se añade la opción independiente «Transiciones de ventana». «Reabrir desde la bandeja» permite elegir «Abrir página predeterminada». El tamaño predeterminado pasa a 1000×680 y el mínimo a 640×480. ([#27](https://github.com/Ray-lea/Argus/issues/27), [#40](https://github.com/Ray-lea/Argus/issues/40))
- **Sustitución del Administrador de tareas**: «Sustituir el Administrador de tareas» añade el botón «Abrir temporalmente el Administrador de tareas de Windows». Este botón abre el Administrador de tareas integrado en Windows. La configuración de sustitución no cambia.
- **Avisos y recordatorios**: La versión portátil explica sus diferencias con la instalada y ofrece enlaces de descarga en la guía de bienvenida y en la esquina inferior derecha de la ventana. El nuevo «Recordatorio de estrella» aparece como máximo una vez al día y se desactiva en «Configuración → Actualizaciones». Las sugerencias situadas encima de «Este PC» se pueden contraer.
- **Canal de Telegram**: «Acerca de → Proyecto y soporte» incluye el nuevo elemento «Canal de Telegram». «Unirse al canal» abre [t.me/argustask](https://t.me/argustask) y «Copiar enlace» copia el enlace del canal.

### Rendimiento

- **Uso de memoria en segundo plano**: La nueva opción «Liberar memoria al ocultar la ventana» libera las imágenes de los gráficos y devuelve la memoria sin usar a Windows tras el tiempo configurado con la ventana minimizada u oculta en la bandeja (10 minutos de forma predeterminada). Con todas las ventanas ocultas o minimizadas, se reduce el uso de CPU de la interfaz. La recopilación de datos, el historial y las reglas siguen funcionando. ([@SpookySandwich](https://github.com/SpookySandwich) [#54](https://github.com/Ray-lea/Argus/pull/54))

### Correcciones

- **Bloqueo del sistema**: Argus ya no consulta la temperatura de los discos conectados a controladoras RAID, como Intel RST. Algunas versiones del controlador no respondían a esta consulta, y el sistema se bloqueaba unos segundos después de iniciar Argus. La temperatura de estos discos se muestra como desconocida. ([#48](https://github.com/Ray-lea/Argus/issues/48))
- **Expulsión segura de discos USB**: La supervisión de discos ya no mantiene los dispositivos en uso de forma continua. Los discos USB se pueden expulsar de forma segura. ([#57](https://github.com/Ray-lea/Argus/issues/57))
- **Lista de discos y datos de rendimiento**: Algunos discos NVMe mostraban solo la información del dispositivo, sin datos de rendimiento. Los discos también desaparecían de la lista cuando los contadores de rendimiento no estaban disponibles. Se corrigen ambos problemas.
- **Detalles del disco**: La tabla SMART mantiene los valores cerca de los nombres de los atributos y evita grandes huecos entre grupos. Los valores sin procesar caben en ventanas estrechas. Las barras de salud usan verde para los valores normales y colores de advertencia según los umbrales de temperatura y capacidad de reserva.
- **Datos de procesos**: Cuando las consultas al sistema son lentas, Argus sigue actualizando la lista de procesos y conserva las últimas lecturas válidas de recursos. Los procesos finalizados ya no permanecen en la lista. Las acciones sobre procesos actualizan el estado mostrado incluso con las actualizaciones en pausa. ([#66](https://github.com/Ray-lea/Argus/issues/66))
- **Tabla de procesos**: Los indicadores del modo de eficiencia y de fijación aparecen junto al nombre. Los encabezados se alinean con las columnas de datos. Los valores de uso de los procesos incluyen el signo de porcentaje.
- **Página Conexiones**: Las conexiones sin proceso asociado se muestran ahora como «Sin proceso asociado».
- **Lecturas de CPU**: Los procesadores de arquitectura híbrida muestran ahora la velocidad correcta. Algunos procesadores Ryzen de seis núcleos muestran ahora la tensión y la temperatura correctas de cada núcleo. Las lecturas de los sensores internos de Intel ya no se interrumpen, y la curva de temperatura ya no se corta. ([#58](https://github.com/Ray-lea/Argus/issues/58))
- **Autorización y desinstalación**: Argus puede revocar la autorización y realizar la desinstalación después de un cambio de permisos en Program Files o en la carpeta de ejecución. Los sensores del kernel pueden usar un controlador PawnIO que ya está en ejecución. La desinstalación ya no deja la carpeta Argus Sensor Setup ni una carpeta Argus Runtime vacía. ([#46](https://github.com/Ray-lea/Argus/issues/46))
- **Permisos de inicio**: Después de la instalación o de una actualización desde la aplicación, Argus intenta primero iniciarse con los permisos de la cuenta del escritorio, como desde un acceso directo. Si se ejecuta como administrador, puede reiniciar un Explorador de archivos que ya se esté ejecutando como administrador y conservar sus permisos.
- **Visualización de la ventana**: Al restaurar el tamaño o iniciar Argus en frío mediante la sustitución del Administrador de tareas, la interfaz ocupa toda la ventana. Windows 11 21H2 usa un fondo sólido. Al cambiar a otra aplicación, la ventana de Argus deja de cubrir las demás. ([#44](https://github.com/Ray-lea/Argus/issues/44), [#37](https://github.com/Ray-lea/Argus/issues/37), [#35](https://github.com/Ray-lea/Argus/issues/35))
- **Bandeja**: Cuando la ventana se cierra y queda oculta en la bandeja, el uso de CPU ya no permanece elevado. Después de una actualización desde la aplicación, el icono de la bandeja ya no pasa al área de desbordamiento. ([#31](https://github.com/Ray-lea/Argus/issues/31))
- **Visualización de la interfaz**: El diálogo de actualización permanece centrado durante la carga y el diálogo de archivos en uso permanece en su posición al mostrar los resultados. En ventanas de poca altura, todo el contenido de «Acerca de» es accesible mediante desplazamiento. Los menús contextuales de los cuadros de texto usan el idioma de la interfaz. ([#39](https://github.com/Ray-lea/Argus/issues/39))
- **Cifras y representación**: Las cifras dinámicas permanecen en su posición al actualizarse. Se corrigen los bordes del texto claro sobre fondos oscuros y los pequeños restos de color bajo el resumen de recursos.
- **Estabilidad**: Argus ya no se cierra cuando la temporización de la pantalla es anómala ni cuando la enumeración de direcciones de los adaptadores de red falla varias veces seguidas. La interfaz ya no deja de responder cuando la recuperación del dispositivo gráfico coincide con el cierre de la ventana.
- **Fuentes grandes, reglas de procesos y elementos de inicio**: Con tamaños de fuente grandes, las lecturas de la página Rendimiento ya no se superponen a las especificaciones. Cuando el nombre de un programa contiene letras no ASCII, el menú de procesos ya no añade reglas duplicadas. Argus ya no se cierra inesperadamente al leer las tareas de inicio de las aplicaciones empaquetadas. ([@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59))
- **Tipo de versión**: Configuración, Acerca de y la barra de estado muestran «Versión de desarrollo», «Beta», «Estable» según el programa en ejecución. El canal de actualizaciones selecciona por separado el origen de las futuras comprobaciones.

### Notas de actualización

- **Servicio privilegiado**: Esta versión actualiza el protocolo de comunicación del servicio a la versión 11. Al actualizar la versión instalada mediante el instalador o desde la aplicación, también se actualizan el servicio instalado y la copia que sustituye al Administrador de tareas. En la versión portátil, después de reemplazar el ejecutable, haga clic en «Actualizar servicio» en «Configuración → Permisos → Servicio privilegiado» y complete la confirmación de administrador.
- **Configuración e historial**: La actualización conserva la configuración y el historial existentes. En el primer inicio, «Transiciones de ventana» adopta el ajuste de transiciones de ventana que tenía «Animaciones de la interfaz».

## [0.4.0](https://github.com/Ray-lea/Argus/compare/v0.3.4...v0.4.0) - 2026-11-06

v0.4.0 migra por completo la interfaz de Tauri, SolidJS y WebView2 a una arquitectura nativa Rust + GPUI Kit, elimina la sobrecarga del motor del navegador, reconstruye el renderizado nativo con DirectX y las interacciones, y optimiza en profundidad las animaciones a altas frecuencias de refresco, los gráficos en tiempo real, las actualizaciones del muestreo de listas y el consumo de recursos en segundo plano. También incorpora columnas de procesos personalizables, salud del disco y detalles SMART, más lecturas de gráficos integrados Intel y nuevas opciones de distribución de ventanas. El [sitio web de Argus](https://argustask.com/es/) ya está disponible, junto con el servidor espejo oficial de actualizaciones.

### Nuevas funciones

- **Columnas de procesos personalizables**: 40 columnas en seis categorías: información general, CPU, memoria, disco e I/O, red y GPU. Se añaden memoria dedicada y compartida de GPU, motor de GPU, nombre de imagen, espacio de direcciones virtuales, máximo conjunto de trabajo, errores de página y contadores acumulados de I/O. El espacio de direcciones virtuales y el máximo conjunto de trabajo se muestran por proceso. El menú contextual de las cabeceras ofrece submenús por categoría y permite marcar opciones sucesivamente; el diálogo «Seleccionar columnas» incluye filtros por categoría, búsqueda y ordenación en dos paneles. Al arrastrar cabeceras o columnas seleccionadas, se reordenan en tiempo real con desplazamientos suaves. La anchura automática reserva espacio según el formato numérico para evitar saltos y también puede ajustarse manualmente. La columna «Nombre» de la lista plana y del árbol de procesos coincide con la vista por aplicaciones: prioriza el nombre de la aplicación o la descripción del archivo y muestra el nombre del ejecutable como texto secundario o en «Nombre de imagen».
- **Salud y detalles del disco**: La página de rendimiento del disco incorpora el selector «Resumen / Detalles». Realiza una lectura puntual, bajo demanda, de dispositivos NVMe, ATA/SATA, USB-SAT y SCSI/SAS, y muestra salud, vida útil restante, temperatura y umbrales, horas y ciclos de encendido, totales de lectura y escritura, errores de medios y de lectura/escritura, enlace PCIe y atributos SMART. Permite repetir la lectura manualmente. El número de serie prioriza el del fabricante obtenido de la identidad del protocolo y se oculta de forma predeterminada.
- **Resumen del sistema y gráficos de rendimiento**: Se rediseña la franja de resumen superior de Procesos. Las curvas ocupan las tarjetas hasta los bordes y se desvanecen bajo las cifras de la izquierda; el límite de escala aparece arriba y toda la tarjeta se resalta al pasar el puntero. Se añaden temperatura de CPU y potencia de CPU y GPU. El resumen de CPU en Rendimiento combina la potencia del paquete y de los núcleos en «Potencia» (prioriza la del paquete, muestra el origen al pasar el puntero y conserva ambas curvas en Detalles). Los gráficos de disco y red ajustan la escala en pasos fijos; las series secundarias de gráficos compactos, como las vistas previas de dispositivos de la barra lateral, usan líneas continuas semitransparentes para mejorar la legibilidad.
- **Lecturas de gráficos integrados Intel**: Se amplían las lecturas de temperatura, frecuencia, voltaje y potencia del controlador. Los sensores del kernel (módulo IntelMCHBAR) y RAPL PP1 añaden temperatura, frecuencia, frecuencia de memoria compartida, VID del núcleo, actividad GT/multimedia y potencia para los gráficos integrados Core de 10.ª generación (Comet Lake S/H). Se distingue el estado de reloj detenido de las secciones gráficas, se muestran potencias inferiores a 1 W y la potencia integrada se incluye en la página de la GPU correspondiente. Las lecturas válidas de RAPL tienen prioridad sobre los ceros de los contadores de potencia de Windows; los datos que no proporcionan el hardware o el controlador permanecen como desconocidos.
- **Ventana y distribución de la interfaz**: «Configuración → Inicio y bandeja» permite elegir la página inicial. El borde derecho de la navegación lateral expandida se puede arrastrar para cambiar su anchura (doble clic para restaurarla). El panel de detalles añade «Mostrar cuando se selecciona un proceso»: se despliega al seleccionar y se recoge al quitar la selección. Al cambiar el intervalo temporal en Historial, se muestra siempre el periodo más reciente hasta el momento actual. El tamaño mínimo pasa a 1240×800 para mantener la distribución completa, con reducción automática si el área disponible de la pantalla principal es menor.
- **Acerca de, novedades y servidor espejo oficial**: «Acerca de» pasa a ser una página de navegación independiente (Ctrl+8), con tarjetas de enlace al sitio de Argus y al repositorio de GitHub. Una insignia de versión a la izquierda de la barra de estado abre los cambios de la versión al hacer clic y permite copiar su número con el botón derecho. Los cambios se agrupan por categoría y las nuevas funciones se presentan en tarjetas de dos columnas. Si falla la conexión a GitHub, la actualización automática prueba primero el servidor espejo oficial de Argus y después los proxies de GitHub; tanto el instalador como las notas de actualización se verifican mediante firma.

### Rendimiento

- **Respuesta de la interfaz y animaciones de alta frecuencia**: Se reduce el trabajo del hilo principal dedicado al muestreo, la medición de texto y el dibujo repetido. Se reutilizan fondos estáticos y capas de composición, y las texturas de curvas grandes se dibujan en paralelo en varios hilos. El uso de GPU de los gráficos disminuye considerablemente con la programación de GPU acelerada por hardware (HAGS) desactivada. Las transiciones de página, el panel de detalles, el desplazamiento de listas y la reordenación de columnas siguen la frecuencia de refresco de la pantalla; las cuadrículas multinúcleo y los gráficos de motores GPU se redimensionan en el mismo fotograma que la ventana.
- **Cambio de página y funcionamiento en segundo plano**: Tras el inicio se precargan las listas de servicios, aplicaciones de inicio y conexiones para mostrarlas en la primera visita. Las preferencias de interfaz se agrupan y se escriben de forma asíncrona en segundo plano, eliminando pausas al cambiar de página. Las páginas sin gráficos en tiempo real omiten reconstrucciones repetidas para reducir el uso de CPU. Al minimizar u ocultar la ventana en la bandeja, se detiene el dibujo y se liberan, según sea necesario, las cachés de capas de gráficos, los iconos caducados y los atlas de texturas, reduciendo memoria y memoria gráfica en segundo plano.
- **Carga elevada y poca memoria**: Un hilo independiente agrupa las actualizaciones de la bandeja para mejorar la respuesta de las operaciones y del muestreo con carga elevada o bloqueos del shell. Cuando escasea la memoria, se reducen automáticamente los búferes y la precisión de las texturas para conservar las curvas en tiempo real; la precisión completa se restaura al recuperarse la memoria disponible. Se mejora la activación de la ventana cuando otras ventanas siempre visibles cubren gran parte de la pantalla.

### Correcciones

- **Tráfico de red por proceso**: Se corrigen lecturas permanentemente a cero en algunos equipos y lecturas que alternaban con cero o se actualizaban tarde con un intervalo de 0,5 s.
- **Servicio privilegiado**: Se corrige el cierre del servicio si un cliente se desconecta inesperadamente antes de completar la conexión de la canalización, así como los tiempos de espera agotados y los falsos resultados sin datos durante lecturas frecuentes.
- **Nombres e iconos de aplicaciones**: Se corrigen las aplicaciones empaquetadas (MSIX) que mostraban la descripción del ejecutable en lugar del nombre usado por el Administrador de tareas, y la ausencia de iconos y descripciones cuando faltaba la correspondencia de rutas de dispositivo o el proceso denegaba su apertura.
- **Memoria gráfica por proceso**: Se usan contadores de memoria residente y se eliminan duplicados por proceso y adaptador físico para corregir valores excesivos o sumados varias veces con ciertos sistemas y controladores. Se distingue entre lecturas ausentes y consumo cero.
- **Potencia de gráficos integrados Intel**: Se corrige la aparición y desaparición repetida de la potencia de los gráficos integrados inactivos en el resumen de CPU.
- **Estado de virtualización de CPU**: Se corrigen los hosts Hyper-V que seguían mostrando «Deshabilitado» con la virtualización de hardware activada.
- **Autorización de inicio como administrador**: Se corrige el fallo de autorización causado por la resolución del SID cuando el nombre de usuario coincide con el del equipo, y se mejora la identificación de copias recompiladas de la misma versión.
- **Memoria en sesiones prolongadas**: Se eliminan iconos duplicados por contenido, se limpia periódicamente la caché de metadatos de programas finalizados y se limita la acumulación de solicitudes cuando el analizador está ocupado. Se corrige así el crecimiento continuo de memoria al iniciar con frecuencia programas desde rutas nuevas.
- **Enlaces externos**: Los enlaces al sitio web, repositorio o página de lanzamiento se entregan directamente al shell del sistema, eliminando la demora de abrir un proceso intermediario del Explorador.

### Notas de actualización

- **Requisitos del sistema**: Windows 10 22H2 (compilación 19045) o posterior, incluido Windows 11, solo x64; dispositivo gráfico y controlador compatibles con DirectX 11. La interfaz deja de depender del entorno de ejecución WebView2.
- **Configuración e historial**: Se conservan los ajustes y archivos de historial. Las actualizaciones desde v0.3.3 o v0.3.4 conservan las preferencias de interfaz guardadas por separado. Al actualizar directamente desde v0.3.2 o anteriores, las preferencias almacenadas únicamente en la antigua caché de WebView2 (distribución, notas de filas y elementos fijados) vuelven a sus valores predeterminados y deben configurarse de nuevo. La nueva versión limpia automáticamente los datos antiguos de WebView2 de Argus en segundo plano.
- **Servicio privilegiado**: El protocolo de comunicación del servicio pasa a la versión 10. Actualizar la versión instalada mediante el instalador o desde la aplicación actualiza también el servicio instalado y la copia que sustituye al Administrador de tareas. Tras reemplazar el ejecutable portátil, es necesario seleccionar «Actualizar servicio» en «Configuración → Permisos → Servicio privilegiado» y completar la confirmación de administrador.

## [0.3.4](https://github.com/Ray-lea/Argus/compare/v0.3.3...v0.3.4) - 2026-10-26

### Correcciones

- **Reinicio del Explorador**: Se corrige la falta de recuperación del escritorio y la barra de tareas cuando el proceso anterior queda bloqueado durante la limpieza de salida o el informe de errores. Si el cierre normal agota el tiempo de espera, solo se termina la instancia anterior identificada y se confirma su salida antes de restaurar el escritorio. Si Windows ya lo ha restaurado, se utiliza la instancia existente para evitar otro inicio. ([#20](https://github.com/Ray-lea/Argus/issues/20))

### Notas de actualización

- La actualización desde v0.3.x conserva los ajustes y el historial existentes.

## [0.3.3](https://github.com/Ray-lea/Argus/compare/v0.3.2...v0.3.3) - 2026-10-26

Esta versión incorpora ajustes de escala de interfaz y tamaño de fuente, mejora el guardado de preferencias de ventanas y listas, y ajusta las estadísticas de uso de GPU.

### Nuevas funciones

- **Escala de interfaz**: «Configuración → Idioma y apariencia» permite una escala del 80 % al 150 %, con un valor predeterminado del 100 % y porcentajes enteros como 113 %.
- **Tamaño de fuente**: El tamaño base se ajusta entre 11 y 20 px, con 13 px como valor predeterminado. Puede combinarse con la escala de interfaz, se aplica inmediatamente y se guarda automáticamente. Ambos ajustes se pueden restablecer por separado.

### Correcciones

- **Estadísticas de GPU**: Se intenta corregir el uso global demasiado bajo con ciertas cargas de cálculo, los picos anómalos al reanudar el muestreo y los totales excesivos con varias GPU o motores. Se añaden contadores de motores por proceso y la utilización del núcleo NVIDIA cuando está disponible, y se muestra el origen de las estadísticas. ([#19](https://github.com/Ray-lea/Argus/issues/19))
- **Restauración de ventana**: Tras cerrar y volver a iniciar, se restauran tamaño, posición y estado maximizado. Cerrar estando minimizada permite abrir normalmente en el siguiente inicio; las ventanas fuera de pantalla vuelven al área visible disponible.
- **Preferencias de listas**: Las listas de procesos, servicios, aplicaciones de inicio y conexiones guardan por separado la columna y el sentido de ordenación. Servicios y Conexiones también restauran los filtros seleccionados.
- **Guardado de preferencias**: Las preferencias de interfaz se guardan por separado y admiten la migración de la caché anterior. Siguen disponibles después de limpiar la caché de interfaz.
- **Distribución y botones de ventana**: La barra de herramientas y la navegación se adaptan a ventanas estrechas con fuentes grandes o escalas altas; se corrige el área de respuesta del botón de maximizar.

### Notas de actualización

- La actualización desde v0.3.x conserva los ajustes y el historial. Las preferencias anteriores de interfaz se migran automáticamente en el primer inicio, sin reconfiguración manual.

## [0.3.2](https://github.com/Ray-lea/Argus/compare/v0.3.1...v0.3.2) - 2026-10-23

Esta versión sustituye a la retirada v0.3.1. Corrige la detección errónea de la versión instalada como portátil al iniciarla como administrador o desde las entradas del Administrador de tareas, que impedía actualizar desde la aplicación.

### Notas de actualización

- Si una copia instalada muestra «La versión portátil no se actualiza automáticamente», la solución es descargar y ejecutar directamente el instalador de esta versión desde su página de lanzamiento, sin desinstalar Argus previamente. Tras actualizar, el inicio como administrador y las entradas del Administrador de tareas siguen admitiendo las actualizaciones integradas.

### Correcciones

- **Actualizaciones de la versión instalada**: Se identifican correctamente las copias de ejecución como administrador y de sustitución del Administrador de tareas, recuperando la búsqueda, descarga e instalación automáticas de actualizaciones.
- **Detección de la versión portátil**: La presencia de un desinstalador junto al programa deja de clasificarla erróneamente como instalada. Se mantiene la descarga manual desde la página de lanzamiento.

### Cambios incluidos

- Incluye el inicio como administrador tras una única autorización de v0.3.1 y las correcciones de reinicio del Explorador, estadísticas de CPU, curvas al restaurar la ventana, cursor de texto en la lista de procesos e instalación de actualizaciones.

## [0.3.1](https://github.com/Ray-lea/Argus/compare/v0.3.0...v0.3.1) - 2026-10-23

**Esta versión se ha retirado. Sus funciones y correcciones están incluidas en v0.3.2.**

Se incorpora el inicio como administrador tras una única autorización, unificando los permisos al iniciar sesión, usar accesos directos y abrir desde el Administrador de tareas. Se corrigen problemas de reinicio del Explorador, uso de CPU e instalación de actualizaciones.

### Notas de actualización

- **Ejecutar como administrador**: Desactivado de forma predeterminada; se activa en «Configuración → Permisos» o en la guía de bienvenida. Requiere una cuenta de administrador. Al activarlo, Argus se reinicia conservando ajustes e historial. El servicio privilegiado, los sensores del kernel y la sustitución del Administrador de tareas mantienen sus autorizaciones independientes.
- **Versión instalada**: Actualizar mediante el instalador o desde la aplicación actualiza también el servicio privilegiado instalado y la copia que sustituye al Administrador de tareas.
- **Versión portátil**: Primero se cierra la versión anterior y después se reemplaza el ejecutable. Si está instalado el servicio privilegiado, se selecciona «Actualizar servicio» en «Configuración → Permisos» de la nueva versión. Antes de posteriores actualizaciones manuales, si el inicio como administrador está activado, se puede desactivar y cerrar Argus, reemplazar el archivo, iniciar desde el ejecutable nuevo y volver a activarlo.

### Nuevas funciones

- **Inicio como administrador**: Tras una autorización, Argus se abre con derechos de administrador al iniciar sesión, desde accesos directos e iconos anclados y, con la sustitución del Administrador de tareas activada, desde Ctrl+Shift+Esc y el menú contextual de la barra de tareas. Configuración y la guía muestran el estado de autorización y los permisos actuales, con opciones para desactivar o reparar la autorización. ([#13](https://github.com/Ray-lea/Argus/issues/13))

### Correcciones

- **Explorador**: Se corrigen fallos de reinicio con derechos estándar o de administrador, anomalías del escritorio tras reiniciar y permisos insuficientes al reiniciar otra vez. La lista de archivos utilizados ofrece «Reiniciar» para el Explorador del escritorio actual, evitando que finalizarlo directamente haga desaparecer el escritorio y la barra de tareas. ([#14](https://github.com/Ray-lea/Argus/issues/14), [#15](https://github.com/Ray-lea/Argus/issues/15))
- **Estadísticas de CPU**: Se corrige el uso de CPU permanentemente al 100 % cuando los contadores de inactividad dejan de actualizarse en algunos sistemas. ([#12](https://github.com/Ray-lea/Argus/issues/12))
- **Curvas**: Se corrigen los datos acumulados que se agrupaban formando picos densos al restaurar la ventana desde la minimización o la bandeja.
- **Lista de procesos**: Se elimina el cursor de texto parpadeante que aparecía al hacer clic en la lista, conservando la búsqueda al escribir y el soporte de métodos de entrada chinos. ([#16](https://github.com/Ray-lea/Argus/issues/16))
- **Instalación de actualizaciones**: Se muestran los procesos que mantienen ocupado el ejecutable y se permite reintentar. Se añaden pasos fallidos y códigos de error, y se corrige la imposibilidad de leer registros de diagnóstico de actualización tras autorizar con otra cuenta de administrador.

### Problemas conocidos

- **Firma de código**: Los ejecutables publicados todavía carecen de firma de código de Windows; Windows SmartScreen puede advertir de un «editor desconocido» en el primer inicio. Las actualizaciones integradas siguen verificándose mediante firmas de actualización.

## [0.3.0](https://github.com/Ray-lea/Argus/compare/v0.2.0...v0.3.0) - 2026-10-22

Se añaden la ejecución de nuevas tareas, el reinicio del Explorador y la copia de especificaciones del equipo, se mejoran los grupos de procesos y las vistas de rendimiento, y se amplía la compatibilidad con Windows 10.

### Cambios incompatibles

- **Historial**: Tras actualizar, Historial empieza con registros nuevos y se reemplaza el archivo anterior del día actual. Para conservar los registros anteriores, es necesario [hacer una copia de la carpeta de historial](https://github.com/Ray-lea/Argus/blob/main/docs/guide/README.md#历史) antes de actualizar.

### Notas de actualización

- **Versión instalada**: Actualizar mediante el instalador o desde la aplicación actualiza también el servicio privilegiado y la copia que sustituye al Administrador de tareas.
- **Versión portátil**: Primero se cierra la versión anterior y se reemplaza el ejecutable. Si está instalado el servicio privilegiado, se selecciona «Actualizar servicio» en «Configuración → Permisos» y se autoriza la operación.

### Nuevas funciones

- **Ejecutar nueva tarea**: Un nuevo acceso en la barra de herramientas permite abrir programas, carpetas, documentos o direcciones web, también como administrador.
- **Reiniciar el Explorador**: El menú contextual del Explorador de Windows en Procesos incluye «Reiniciar».
- **Atajos de teclado**: Mantener Ctrl pausa las actualizaciones y soltarlo las reanuda. Alt+E termina el proceso seleccionado en Procesos. Configuración incluye una referencia de atajos.
- **Grupos de procesos**: La vista por aplicaciones se divide en Aplicaciones, Procesos en segundo plano y Procesos de Windows, con cabeceras plegables y uso total por grupo.
- **Búsqueda rápida**: Escribir directamente en Procesos inicia una búsqueda, con soporte de métodos de entrada chinos. Los filtros activos se señalan con el color de énfasis.
- **Especificaciones del equipo**: El panel de detalles permanece abierto de forma predeterminada y permite ajustar su anchura. Sin un proceso seleccionado, muestra el hardware del equipo, que puede ocultarse o copiarse como texto.
- **Página de CPU**: Se divide en Resumen y Detalles. Los gráficos de núcleos reorganizan su distribución con transiciones suaves al cambiar el tamaño de la ventana.
- **Página de GPU**: Muestra automáticamente los motores activos; también permite elegir motores, cambiar la distribución de gráficos y ajustar su altura.
- **Composición de memoria**: Se añade la memoria modificada. Al pasar el puntero por los iconos de la leyenda se muestran explicaciones.
- **Historial de batería**: Historial incorpora curvas de nivel de batería y potencia de carga y descarga para compararlas con el uso de recursos.
- **Elementos fijados y notas**: Los menús contextuales de Servicios, Inicio y Conexiones permiten fijar entradas y añadir notas.
- **Animaciones de ventana**: Minimizar, restaurar y mostrar u ocultar desde la bandeja utiliza un fundido en la misma posición.
- **Notas de actualización**: Configuración y los diálogos comparten distribución, admiten notas en inglés y adaptan la altura del diálogo a la ventana.
- **Registros de diagnóstico**: Se añaden pasos fallidos, códigos de error, rutas pertinentes y modelos de CPU y GPU para facilitar los informes de problemas.
- **Acerca de**: Se añade un enlace a la página principal del proyecto.

### Correcciones

- **Estadísticas de CPU**: Se corrige la infravaloración del uso total, por procesador lógico y por proceso durante cargas breves.
- **Windows 10**: Se corrigen el escritorio visible a través del fondo de la ventana, las ventanas vacías de configuración del sistema y los datos de lectura y escritura de disco ausentes por proceso.
- **Sustitución del Administrador de tareas**: Se corrigen fallos al activarla y avisos erróneos de que el programa se estaba iniciando o cerrando en ciertos entornos. Se añaden avisos sobre bloqueos del software de seguridad.
- **Sensores del kernel**: Se corrigen la detección y actualización de versiones antiguas de PawnIO y el estado activado cuando el controlador falla.
- **Aplicaciones de inicio**: Se completa la visualización, activación y desactivación de entradas de aplicaciones empaquetadas y se corrigen sus nombres y estados.
- **Inicio del programa**: Se corrige el fallo al abrir la ventana de Argus tras autorizar con otra cuenta de administrador o cuando el Explorador ya se ha cerrado.
- **Identificación de procesos**: Se corrigen la identificación de la aplicación propietaria de WebView2, la fusión errónea de programas con el mismo nombre y las agrupaciones incorrectas con la ordenación pausada.
- **Entrada en chino**: Se corrige la pérdida del primer carácter escrito al volver a Procesos.
- **Curvas**: Se corrigen el recorte progresivo por la izquierda, los saltos del punto de lectura al pasar el puntero y los saltos de las curvas después de una pausa. Se mejora la transición al cambiar el límite superior.
- **Distribución de páginas**: Se corrigen los saltos de distribución de páginas y diálogos al aparecer la barra de desplazamiento.
- **Guardado del historial**: Se corrige la pérdida de datos registrados cuando falla la lectura del archivo del día actual.
- **Refuerzo de seguridad**: Se actualizan componentes básicos y se refuerzan las restricciones de carga de contenido externo en las páginas integradas.

### Rendimiento

- Se mejora el desplazamiento de la lista de procesos y se reducen las actualizaciones repetidas en segundo plano y su consumo de recursos.

### Problemas conocidos

- **Firma de código**: El certificado oficial está en trámite; Windows SmartScreen puede advertir de un «editor desconocido» en el primer inicio.

Gracias a @kostyanp95 por contribuir el historial de batería.

## [0.2.0](https://github.com/Ray-lea/Argus/compare/v0.1.0...v0.2.0) - 2026-10-18

Esta versión incorpora velocidades de lectura y escritura de disco por proceso, permite configurar intervalos distintos al minimizar y ocultar en la bandeja, mejora las actualizaciones integradas y la configuración de atajos, y corrige, entre otros problemas, cierres inesperados tras un tiempo de ejecución en algunos equipos. Con el servicio privilegiado instalado, conviene leer primero «Cambios incompatibles».

### Cambios incompatibles

- **El servicio privilegiado requiere actualización**: Cambia la comunicación entre Argus y el servicio privilegiado. El servicio de 0.1.0 no es compatible con la nueva versión. Las instalaciones sin este servicio no se ven afectadas.
- **Actualización mediante el instalador**: Incluye las actualizaciones integradas y la ejecución directa del instalador. Este cierra primero las copias antiguas de Argus que siguen abiertas desde la sustitución del Administrador de tareas y actualiza el servicio privilegiado con la misma autorización de administrador, sin pasos adicionales. Si «Sustituir el Administrador de tareas» está activado, permanece así y, tras actualizar correctamente, Ctrl+Shift+Esc abre la nueva versión.
- **Actualización de la versión portátil**: Si está instalado el servicio privilegiado, debe actualizarse manualmente después de reemplazar el ejecutable. Basta con seleccionar «Actualizar servicio» en el aviso de la nueva versión de Argus o en «Configuración → Permisos» y autorizar. El mismo procedimiento sirve si el instalador no consigue actualizar el servicio. Hasta actualizarlo, los datos de red, disco y sensores de CPU que dependen de él no están disponibles y las operaciones administrativas que requieren elevación pueden fallar. Con «Sustituir el Administrador de tareas» activado, Ctrl+Shift+Esc todavía puede abrir la versión anterior; actualizar el servicio actualiza también el programa de sustitución y mantiene la función activada.

### Nuevas funciones

- **Lectura y escritura de disco por proceso**: Procesos incorpora «Lectura de disco» y «Escritura en disco», dos columnas ordenables con los datos leídos y escritos por segundo. La vista por aplicaciones muestra la suma del grupo y el panel de detalles también incluye estos valores. Las columnas I/O incluyen además operaciones de red y dispositivos y se contabilizan por separado. Windows 11 24H2 y posteriores no requieren permisos adicionales; las versiones anteriores necesitan el servicio privilegiado o ejecutar como administrador, y la explicación de autorización del servicio ya lo indica. Sin lecturas disponibles se muestra «—» y el motivo aparece al pasar el puntero. En ventanas estrechas se conservan ambas columnas y la tabla se desplaza horizontalmente con Nombre fijo a la izquierda; por ello, las columnas de hilos, identificadores, PID e I/O se ocultan antes que anteriormente.
- **Actualizaciones integradas**: A partir de esta versión, tras descargar una nueva versión y verificar su firma, aparece un botón a la derecha de la barra de título, un punto en el icono de la bandeja y un aviso único en la esquina inferior derecha por versión. La versión portátil avisa al detectar una versión nueva y no la descarga automáticamente. El diálogo agrupa por categoría los cambios de todas las versiones entre la actual y la nueva e indica el número de cambios de cada una. Las notas se verifican mediante firma igual que el instalador. El menú contextual del botón permite «No volver a avisar de esta versión». Las actualizaciones descargadas se conservan al reiniciar Argus.
- **Mostrar novedades después de actualizar**: Al abrir la ventana por primera vez tras una actualización se muestran sus cambios. «Mostrar novedades después de actualizar» puede desactivarse en «Configuración → Actualizaciones»; después solo aparece un aviso de actualización en la esquina inferior derecha. «Ver cambios de esta versión», en la misma página, permite abrirlos en cualquier momento.
- **Intervalo en segundo plano**: Los intervalos al minimizar y ocultar en la bandeja se configuran por separado entre 0,5 y 30 s, con 5 s como valor predeterminado, en «Configuración → Datos e historial» y en la guía de bienvenida. Por debajo de 5 s se avisa previamente del aumento de CPU y consumo energético; por encima de 5 s, el historial registra menos muestras. Al volver al primer plano se actualiza inmediatamente. Si el intervalo de fondo supera al del primer plano, aparece un aviso único abajo a la derecha con acceso a Configuración y opción de no volver a avisar.
- **Configuración de atajos**: Permite desactivar un atajo conservando su combinación y recuperarlo al activarlo de nuevo. La grabación muestra en tiempo real las teclas pulsadas, explica las combinaciones no disponibles y ofrece «Restaurar valor predeterminado». Las combinaciones como Ctrl+letra o Alt+letra, que pueden entrar en conflicto con otros programas, se guardan tras confirmación. Si otro programa ocupa la combinación, se puede reintentar directamente; al cerrarlo, Argus también reintenta automáticamente al volver a mostrar la ventana.
- **Tiempo de actividad de CPU**: Se muestra con precisión de segundos, por ejemplo «5 días 14:03:07», y cambia en cada actualización.

### Correcciones

- **Cierres inesperados**: Se corrige el posible cierre de Argus después de un tiempo de ejecución en ciertos equipos sin gráficos Intel disponibles.
- **Pérdida de ajustes**: Se corrige la pérdida de configuración tras un corte de corriente o reinicio forzado poco después de guardarla, que hacía reaparecer la guía de bienvenida y restablecía el idioma según el sistema.
- **Inicio automático**: Deja de quitar el foco a la ventana activa. Antes, la entrada de teclado iba a la ventana invisible de Argus hasta hacer clic en otro lugar. Oculta en la bandeja, la interfaz deja de dibujarse continuamente en segundo plano, reduciendo considerablemente CPU y GPU y más de la mitad de la memoria.
- **Entrada de inicio automático**: Activar «Iniciar al iniciar sesión en Windows» desde Argus abierto con Ctrl+Shift+Esc hace que la entrada apunte al Argus instalado. Las entradas que apuntaban a la copia de sustitución se corrigen automáticamente al abrir una vez la nueva versión desde el menú Inicio u otro acceso equivalente.
- **Actualización en segundo plano**: El intervalo real al minimizar u ocultar en la bandeja coincide con el configurado. Antes, un intervalo de 5 s solía durar 6 s y a veces 10 s. Con derechos estándar y el servicio privilegiado, las columnas de red de Procesos dejan de desaparecer brevemente al restaurar la ventana desde la bandeja o desde el estado minimizado.
- **Ventana**: Se eliminan los dos grupos de botones superpuestos arriba a la derecha al maximizar. La ventana puede arrastrarse con el puntero pegado al borde superior de la pantalla.
- **Instalador y desinstalador**: «Cerrar» en la página final, «Cancelar» en la confirmación de desinstalación y la fila de la casilla quedan centrados; antes estaban algo desplazados a la izquierda.
- **Actualización con instalador**: Ejecutar directamente el instalador cierra también las copias antiguas de Argus abiertas mediante Ctrl+Shift+Esc o entradas antiguas de inicio automático, evitando que los inicios posteriores a la actualización se transfieran a la versión antigua. Ya no hace falta salir previamente desde la bandeja.
- **Desinstalación**: También se cierra el Argus abierto mediante Ctrl+Shift+Esc. Antes permanecía en segundo plano y, tras marcar «Eliminar también ajustes, historial y registros» y reinstalar, la guía inicial no aparecía y se volvían a guardar los ajustes antiguos. Si quedan datos sin eliminar, el desinstalador enumera las carpetas restantes.
- **Guía de bienvenida**: Mantener pulsado Ctrl+Shift+Esc al abrir Argus deja de hacer que Esc cierre directamente la guía y la marque como completada.
- **Grabación de atajos**: Pulsar la combinación actual o mantener una tecla durante más tiempo deja de ocultar la ventana accidentalmente. En distribuciones de teclado distintas de la estadounidense, la tecla grabada coincide con la que activa el atajo. Shift+Backspace y Win+Backspace dejan de borrar la combinación por error.
- **Combinaciones de atajos**: Las combinaciones nuevas deben incluir Ctrl, Alt o Win, salvo algunas teclas como F13–F24. Se rechazan combinaciones como Shift+letra que interfieren con la escritura y acciones habituales de edición o del sistema como Ctrl+C y Alt+F4. Las combinaciones ya guardadas se conservan.
- **Atajos**: Se corrigen teclas del teclado numérico registradas como otras teclas. Un fallo al guardar un cambio deja de crear un atajo adicional imposible de cancelar. Los motivos de fallo de registro se muestran en el idioma de la interfaz. En la guía de bienvenida, un atajo ocupado o no disponible muestra únicamente el motivo y la indicación de que puede cambiarse en Configuración.
- **Sustituir el Administrador de tareas**: Al actualizar el servicio privilegiado, la sustitución permanece activada y su copia de Argus se actualiza; se elimina el paso de restaurar el Administrador de tareas de Windows y reactivar manualmente la sustitución. Si la activación falla, se indica el paso rechazado y la posible causa, como el bloqueo del registro por software de seguridad. «Configurado por otro programa» enumera las configuraciones concretas. Tras desactivar la sustitución, herramientas como Process Explorer que asuman el Administrador de tareas dejan de impedir actualizar o desinstalar el servicio. Los fallos de activación ya no dejan copias adicionales en la carpeta del servicio; las copias antiguas se limpian al actualizar, activar o desactivar la sustitución, o actualizar o desinstalar el servicio, salvo que estén en ejecución.
- **Procesos**: Al pulsar «Mostrar detalles» sin selección se solicita seleccionar primero una entrada; antes no ocurría nada. Con un menú contextual o confirmación abierto, la barra de estado deja de mostrar «Clasificación en pausa mientras el puntero está sobre la lista» y la lista no se reordena al aparecer la confirmación. Cambiar de página mediante un atajo con el puntero sobre la lista deja de conservar ese aviso en otras páginas.
- **Gráficos**: El punto de lectura deja de dar saltos con el puntero cerca del borde derecho del gráfico.
- **Cambio de canal de actualizaciones**: Cambiar entre Estable y Beta durante una comprobación o descarga inicia otra comprobación en el canal nuevo, en lugar de ofrecer la versión del anterior.
- **Registro de diagnóstico**: Los fallos incluyen el código de error del sistema en lugar del genérico operation failed, que antes aparecía en casi todos los casos en sistemas no ingleses. Al iniciar se registra también el motivo por el que no se pudo conectar al servicio privilegiado, como una versión incompatible o un tiempo de espera agotado.

### Rendimiento

- **Página de procesos**: Se reduce el coste de dibujar las pequeñas barras de uso por procesador lógico en el resumen del sistema. Con las curvas en movimiento, la interfaz envía aproximadamente un tercio menos de datos a la GPU por fotograma. Las lecturas y escrituras de WebView2 en I/O proceden principalmente de esos datos, por lo que también disminuyen.

### Limitaciones conocidas

- **Lectura y escritura de disco**: El método de Windows 11 24H2 y posteriores solo se ha verificado en la compilación 26300. Las versiones anteriores utilizan el servicio privilegiado o derechos de administrador y todavía no se han verificado en esos sistemas. Si ambas columnas siempre muestran «—», el informe puede incluir el registro de diagnóstico, donde queda anotado el motivo del fallo de lectura.
- **Firma de código**: El programa aún carece de firma de código; Windows SmartScreen puede advertir de un «editor desconocido» en el primer inicio.
- **Otros**: Las demás limitaciones figuran en el documento de [problemas conocidos](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md) de GitHub.

## 0.1.0 - 2026-10-16

Primera versión pública de Argus.

### Nuevas funciones

- **Procesos**: Vistas por aplicación, árbol o lista; la ordenación por nombre separa «Aplicaciones» y «Procesos en segundo plano». Muestra CPU, memoria, I/O y GPU por proceso, además del tráfico de red con el servicio privilegiado o derechos de administrador. Permite buscar y terminar procesos o árboles completos. El panel de detalles muestra línea de comandos, usuario, nivel de integridad y otros datos; al agrupar por aplicación, presenta los totales de toda ella.
- **Prioridad y modo de eficiencia**: Se ajustan desde el menú contextual o mediante reglas que se aplican automáticamente cada vez que se inicia el programa.
- **Rendimiento**: CPU (por núcleo, núcleos de rendimiento y eficiencia, frecuencia, temperatura, potencia y estados de inactividad), memoria, discos, red, GPU (con potencia en vatios para NVIDIA, AMD e Intel) y batería. Las curvas fluyen a la frecuencia de refresco de la pantalla y se adaptan a la ventana; la información menos frecuente se recoge en tarjetas desplegables.
- **Sensores del kernel opcionales**: Al activarlos en Configuración o en la guía de bienvenida, se leen los sensores internos de CPU mediante el servicio privilegiado y el controlador oficial PawnIO: temperatura y voltaje por núcleo, potencia y estados de límite, datos AMD SMU y velocidades de ventilador, voltajes y temperaturas de placas base compatibles.
- **Servicios y aplicaciones de inicio**: Inicio y detención de servicios y modificación de su tipo de inicio; activación y desactivación de aplicaciones de inicio mediante el mismo interruptor que el Administrador de tareas.
- **Conexiones y puertos**: Puntos de conexión TCP y UDP con sus procesos, búsqueda exacta por puerto y cierre de conexiones TCP IPv4.
- **Archivos utilizados**: Arrastrar un archivo o carpeta a la ventana busca los procesos que lo usan mediante Restart Manager y enumeración de identificadores, incluido el directorio actual de los procesos.
- **Historial**: Registra los últimos 7 días de uso de recursos. Un clic en cualquier instante muestra los procesos con mayor consumo en ese momento.
- **Bandeja del sistema**: Ctrl+Alt+Esc muestra u oculta la ventana de forma global, con consumo de recursos muy bajo mientras permanece oculta.
- **Servicio privilegiado opcional**: Tras una autorización de administrador, la interfaz con derechos estándar puede gestionar procesos del sistema, aplicar reglas automáticamente, medir tráfico de red por proceso, controlar servicios y cerrar conexiones.
- **Sustitución del Administrador de tareas**: Ctrl+Shift+Esc y otros accesos pueden abrir Argus. Se incluye una herramienta de recuperación independiente de la interfaz y del servicio.
- **Actualizaciones integradas**: Canales Estable y Beta, con proxy de GitHub opcional. Solo se instalan versiones que superan la verificación de firma.
- **Apariencia**: La barra de herramientas sirve también de barra de título y la navegación se sitúa debajo o a la izquierda de la ventana. Fondo Mica y color de énfasis del sistema; los temas claro y oscuro siguen Windows de forma predeterminada y pueden elegirse en Configuración.
- **Instalación y desinstalación**: Instalador de una página que sigue el modo claro u oscuro de Windows. Actualiza en el mismo lugar las versiones instaladas y permite eliminar ajustes, historial y registros al desinstalar.
- **Idiomas y guía de bienvenida**: Interfaz, bandeja e instalador en chino simplificado e inglés, siguiendo de forma predeterminada el idioma de Windows. La guía inicial configura idioma, visualización y actualización de datos, permanencia en la bandeja, actualizaciones y otras opciones habituales; también permite instalar directamente el servicio privilegiado opcional, activar sensores del kernel o sustituir el Administrador de tareas.

### Limitaciones conocidas

- El programa aún carece de firma de código; Windows SmartScreen puede advertir de un «editor desconocido» en el primer inicio.
- Los sensores internos de CPU requieren activar los sensores del kernel (servicio privilegiado y controlador PawnIO). Los procesadores y placas base compatibles se enumeran en [datos de hardware](https://github.com/Ray-lea/Argus/blob/main/docs/guide/hardware-data.md#消费级硬件兼容范围).
- Otras limitaciones figuran en [problemas conocidos](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md).
